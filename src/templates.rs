pub struct Snippet {
    pub name: &'static str,
    pub description: &'static str,
    pub code: &'static str,
}

pub const SNIPPETS: &[Snippet] = &[
    Snippet {
        name: "Leaderstats & Currency",
        description: "Creates player leaderstats with Coins and Level",
        code: r#"local Players = game:GetService("Players")

local function onPlayerAdded(player: Player)
	local leaderstats = Instance.new("Folder")
	leaderstats.Name = "leaderstats"
	leaderstats.Parent = player

	local coins = Instance.new("IntValue")
	coins.Name = "Coins"
	coins.Value = 100
	coins.Parent = leaderstats

	local level = Instance.new("IntValue")
	level.Name = "Level"
	level.Value = 1
	level.Parent = leaderstats
end

Players.PlayerAdded:Connect(onPlayerAdded)
"#,
    },
    Snippet {
        name: "DataStore Template",
        description: "Safely loads and saves player data with pcalls",
        code: r#"local DataStoreService = game:GetService("DataStoreService")
local Players = game:GetService("Players")
local myDataStore = DataStoreService:GetDataStore("PlayerData_v1")

Players.PlayerAdded:Connect(function(player)
	local key = "Player_" .. player.UserId
	local success, data = pcall(function()
		return myDataStore:GetAsync(key)
	end)

	if success and data then
		print("Loaded data for:", player.Name, data)
	else
		print("New player joined:", player.Name)
	end
end)

Players.PlayerRemoving:Connect(function(player)
	local key = "Player_" .. player.UserId
	local leaderstats = player:FindFirstChild("leaderstats")
	local coins = leaderstats and leaderstats:FindFirstChild("Coins")
	local val = coins and coins.Value or 0

	pcall(function()
		myDataStore:SetAsync(key, { Coins = val })
	end)
end)
"#,
    },
    Snippet {
        name: "RemoteEvent Server & Client",
        description: "Network communication template",
        code: r#"local ReplicatedStorage = game:GetService("ReplicatedStorage")
local remoteEvent = ReplicatedStorage:FindFirstChild("MyRemote") or Instance.new("RemoteEvent")
remoteEvent.Name = "MyRemote"
remoteEvent.Parent = ReplicatedStorage

-- Server listener:
remoteEvent.OnServerEvent:Connect(function(player, action, ...)
	print("Received from " .. player.Name .. ":", action)
	-- Process server action here
end)
"#,
    },
    Snippet {
        name: "KillBrick / Lava Hazard",
        description: "Damages player humanoid on touch",
        code: r#"local part = script.Parent

local function onTouched(otherPart: BasePart)
	local character = otherPart.Parent
	local humanoid = character and character:FindFirstChildOfClass("Humanoid")
	if humanoid and humanoid.Health > 0 then
		humanoid:TakeDamage(100)
	end
end

part.Touched:Connect(onTouched)
"#,
    },
    Snippet {
        name: "TweenService UI / Object Animation",
        description: "Smoothly animates position, size, or transparency",
        code: r#"local TweenService = game:GetService("TweenService")
local target = script.Parent

local tweenInfo = TweenInfo.new(
	1.0,                           -- Duration in seconds
	Enum.EasingStyle.Quad,         -- EasingStyle
	Enum.EasingDirection.Out,      -- EasingDirection
	0,                             -- RepeatCount (-1 for infinite)
	false,                         -- Reverses
	0                              -- Delay
)

local goal = {
	Position = Vector3.new(0, 10, 0),
	Transparency = 0.5,
}

local tween = TweenService:Create(target, tweenInfo, goal)
tween:Play()
"#,
    },
    Snippet {
        name: "Raycast Query",
        description: "Performs physics raycasting from point A to B",
        code: r#"local origin = script.Parent.Position
local direction = Vector3.new(0, -50, 0)

local raycastParams = RaycastParams.new()
raycastParams.FilterType = Enum.RaycastFilterType.Exclude
raycastParams.FilterDescendantsInstances = { script.Parent }
raycastParams.IgnoreWater = true

local result = workspace:Raycast(origin, direction, raycastParams)
if result then
	print("Hit instance:", result.Instance.Name)
	print("Hit position:", result.Position)
	print("Hit normal:", result.Normal)
end
"#,
    },
    Snippet {
        name: "OOP ModuleScript Boilerplate",
        description: "Object-oriented class with constructor and methods",
        code: r#"local CustomClass = {}
CustomClass.__index = CustomClass

function CustomClass.new(name: string, value: number)
	local self = setmetatable({}, CustomClass)
	self.Name = name
	self.Value = value or 0
	return self
end

function CustomClass:DoSomething()
	print(self.Name, "is executing action with value:", self.Value)
end

function CustomClass:Destroy()
	setmetatable(self, nil)
end

return CustomClass
"#,
    },
    Snippet {
        name: "Day / Night Cycle",
        description: "Rotates clock time dynamically",
        code: r#"local Lighting = game:GetService("Lighting")
local RunService = game:GetService("RunService")

local MINUTES_PER_SECOND = 1

RunService.Heartbeat:Connect(function(dt)
	Lighting.ClockTime = (Lighting.ClockTime + (MINUTES_PER_SECOND * dt / 60)) % 24
end)
"#,
    },
    Snippet {
        name: "ProximityPrompt Interaction",
        description: "Interactive object prompt with hold duration",
        code: r#"local prompt = script.Parent:FindFirstChildOfClass("ProximityPrompt")
	or Instance.new("ProximityPrompt", script.Parent)

prompt.ActionText = "Interact"
prompt.ObjectText = "Machine"
prompt.HoldDuration = 0.5
prompt.MaxActivationDistance = 10

prompt.Triggered:Connect(function(player)
	print(player.Name, "triggered interaction!")
end)
"#,
    },
];

pub struct ToolboxPreset {
    pub name: &'static str,
    pub category: &'static str,
    pub icon: &'static str,
    pub description: &'static str,
    pub class: &'static str,
    pub default_script: Option<(&'static str, &'static str)>, // (name, code)
}

pub const TOOLBOX_PRESETS: &[ToolboxPreset] = &[
    ToolboxPreset {
        name: "Leaderstats System",
        category: "Scripting",
        icon: "🏆",
        description: "ServerScriptService coins & level stats setup",
        class: "Script",
        default_script: Some(("Leaderstats", SNIPPETS[0].code)),
    },
    ToolboxPreset {
        name: "KillBrick Hazard",
        category: "Gameplay",
        icon: "🔥",
        description: "Lethal red obstacle part with touch damage",
        class: "Part",
        default_script: Some(("DamageScript", SNIPPETS[3].code)),
    },
    ToolboxPreset {
        name: "Checkpoint Spawner",
        category: "Gameplay",
        icon: "🚩",
        description: "Obby checkpoint spawn pad",
        class: "SpawnLocation",
        default_script: None,
    },
    ToolboxPreset {
        name: "Interactive Door",
        category: "Interactive",
        icon: "🚪",
        description: "ProximityPrompt animated door model",
        class: "Model",
        default_script: Some(("DoorController", SNIPPETS[8].code)),
    },
    ToolboxPreset {
        name: "Day / Night Atmosphere",
        category: "Environment",
        icon: "☀️",
        description: "Dynamic 24h lighting cycle script",
        class: "Script",
        default_script: Some(("DayNightCycle", SNIPPETS[7].code)),
    },
    ToolboxPreset {
        name: "Main GUI Framework",
        category: "UI",
        icon: "📱",
        description: "ScreenGui canvas with centered container Frame",
        class: "ScreenGui",
        default_script: None,
    },
];
