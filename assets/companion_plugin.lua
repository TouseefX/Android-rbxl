--!strict
-- rbxl Editor Companion Plugin
--
-- Install:  place this file (or the compiled .rbxm) in your Studio Plugins
-- folder (Studio → Plugins tab → "Plugins Folder" button). It connects a
-- WebSocket back to the rbxl Editor app running on the same machine so the
-- editor can run commands in the *real* engine, read selection, sync DOM
-- changes, etc. The protocol is plain JSON:
--
--   { "id": <n>, "method": "<name>", "params": { ... } }
--
-- Notifications have no "id"; requests have one and we reply with the same id.

local HttpService = game:GetService("HttpService")
local Selection = game:GetService("Selection")
local ChangeHistoryService = game:GetService("ChangeHistoryService")
local RunService = game:GetService("RunService")
local StudioService = game:GetService("StudioService")

local HOST = "127.0.0.1"
local PORT = 41742
local URL = ("ws://%s:%d/ws"):format(HOST, PORT)

local toolbar = plugin:CreateToolbar("rbxl Editor")
local toggleButton = toolbar:CreateButton(
	"Live Session",
	"Toggle connection to the rbxl Editor app",
	"rbxassetid://14053512762"
)
toggleButton:SetActive(false)

local conn: WebSocket? = nil
local connected = false
local nextId = 0
local pending: { [number]: (any) -> () } = {}

local function setStatus(active: boolean, label: string?)
	connected = active
	toggleButton:SetActive(active)
	toggleButton.ToolbarText = label or (active and "Connected" or "Disconnected")
end

local function encode(msg: any): string
	return HttpService:JSONEncode(msg)
end

local function decode(s: string): any
	local ok, v = pcall(HttpService.JSONDecode, HttpService, s)
	if ok then return v end
	return nil
end

-- Send a request/notification to the editor.
local function send(method: string, params: any?)
	if not conn then return end
	nextId += 1
	local msg = { id = nextId, method = method, params = params or {} }
	task.spawn(function()
		local ok, err = pcall(function()
			(conn :: WebSocket):Send(encode(msg))
		end)
		if not ok then
			warn("[rbxl-companion] send failed: " .. tostring(err))
		end
	end)
end

local function reply(id: number, result: any, err: string?)
	send("response", { id = id, result = result, error = err })
end

-- Built-in RPC handlers. Add more here as the editor needs them.
local handlers = {}

handlers.ping = function(_: any): any
	return { pong = true, place = game.Name, time = os.time() }
end

handlers.run_command = function(params: any): any
	local source = tostring(params and params.source or "")
	-- Compile the expression; if it returns a value, surface it back.
	local chunk, compileErr = loadstring(source, "=rbxl-command")
	if not chunk then
		return { ok = false, error = tostring(compileErr) }
	end
	local results = { pcall(chunk) }
	local ok = table.remove(results, 1)
	return { ok = ok, results = results }
end

handlers.get_selection = function(_: any): any
	local names = {}
	for _, inst in ipairs(Selection:Get()) do
		table.insert(names, inst:GetFullName())
	end
	return names
end

handlers.set_selection = function(params: any): any
	local target = params and params.path
	if type(target) == "string" then
		-- Very small path resolver: "game.Workspace.Part" → instance
		local obj: Instance? = game
		for seg in string.gmatch(target, "[^%.]+") do
			if obj == nil then break end
			if seg == "game" then
				obj = game
			else
				obj = (obj :: Instance):FindFirstChild(seg)
			end
		end
		if obj then
			Selection:Set({ obj })
			return { ok = true }
		end
	end
	return { ok = false, error = "path not found" }
end

handlers.undo = function(_: any)
	ChangeHistoryService:Undo()
	return { ok = true }
end
handlers.redo = function(_: any)
	ChangeHistoryService:Redo()
	return { ok = true }
end

handlers.get_place = function(_: any): any
	-- Return a minimal tree summary; the editor can ask for richer data
	-- in later versions.
	local function walk(inst: Instance, depth: number): any
		if depth > 4 then return nil end
		local children = {}
		for _, c in ipairs(inst:GetChildren()) do
			table.insert(children, {
				name = c.Name,
				class = c.ClassName,
				children = walk(c, depth + 1),
			})
		end
		return children
	end
	return { name = game.Name, children = walk(game, 0) }
end

-- Main receive loop for one connection.
local function listen(ws: WebSocket)
	while true do
		local raw, err = ws.Receive()
		if not raw then
			warn("[rbxl-companion] disconnected: " .. tostring(err))
			return
		end
		local msg = decode(raw)
		if type(msg) == "table" and type(msg.method) == "string" then
			if msg.method == "response" then
				local id = msg.params and msg.params.id
				if id and pending[id] then
					pending[id](msg.params.result)
					pending[id] = nil
				end
			elseif handlers[msg.method] then
				local ok, result = pcall(handlers[msg.method], msg.params or {})
				if msg.id ~= nil then
					reply(msg.id, if ok then result else nil, if ok then nil else tostring(result) end)
				end
			else
				if msg.id ~= nil then
					reply(msg.id, nil, "unknown method: " .. msg.method)
				end
			end
		end
	end
end

local function connect()
	if conn then
		pcall(function() (conn :: WebSocket):Close() end)
		conn = nil
	end

	local ok, ws = pcall(function()
		return HttpService:WebSocketConnect(URL)
	end)
	if not ok then
		setStatus(false, "Connect failed")
		warn("[rbxl-companion] could not connect to editor at " .. URL .. ": " .. tostring(ws))
		return false
	end

	conn = ws
	setStatus(true, "Connected")
	send("hello", {
		place = game.Name,
		placeId = game.PlaceId,
		userId = StudioService:GetUserId(),
	})

	task.spawn(function()
		listen(ws :: WebSocket)
		setStatus(false, "Disconnected")
		conn = nil
	end)
	return true
end

toggleButton.Click:Connect(function()
	if connected then
		if conn then
			pcall(function() (conn :: WebSocket):Close() end)
		end
		conn = nil
		setStatus(false, "Disconnected")
	else
		connect()
	end
end)

-- Try to auto-connect shortly after Studio loads.
task.delay(1.5, function()
	if not connected then
		connect()
	end
end)

setStatus(false, "Idle")
