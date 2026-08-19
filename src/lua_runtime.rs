//! A tiny embedded **Luau** runtime for testing scripts and running plugins.
//!
//! This is NOT a full Roblox engine — there's no physics simulation or live
//! game server. It's a sandboxed Luau VM (the real, full-accuracy Luau
//! interpreter from TouseefX/luaur, pure Rust + Android-ready) preloaded with
//! the host surface that plugins and pure-logic ModuleScripts need:
//!
//! * `print`/`warn`/`error`/`pcall`/`xpcall`, `typeof`, `tostring`
//! * `task.wait/spawn/defer/delay` (synchronous stubs)
//! * Minimal `Vector3`/`Vector2`/`Color3`/`CFrame`/`UDim2` with metatables
//! * A permissive `Enum` table (any `Enum.Foo.Bar` returns a table with
//!   `Name`/`Value`/`EnumType`)
//! * An `Instance.new` stub with `.Name/.ClassName/.Parent` and no-op methods
//! * `plugin:CreateToolbar/CreateButton/CreateDockWidgetPluginGui/GetSetting/
//!   SetSetting`, and a `script` placeholder so plugin entry points load.
//!
//! Plugin GUI widgets created via `CreateDockWidgetPluginGui` are returned as
//! `Instance`-like tables; the editor's plugin manager can walk their
//! descendant tree for a GUI preview, but the widgets don't render inside the
//! editor (that would need a full Roblox UI engine). For live DataModel access
//! (real services, physics, rendering) you still need Roblox Studio; this VM
//! is for running the *logic* of plugins and scripts offline.

use luaur::{
    Error as LuaError, Function, Lua, MultiValue, Result as LuaResult, Table, Value, Variadic,
};
use std::cell::RefCell;

/// Result of running a script: captured output + success flag.
#[derive(Debug, Clone)]
pub struct RunResult {
    pub success: bool,
    pub lines: Vec<OutputLine>,
}

#[derive(Debug, Clone)]
pub struct OutputLine {
    pub level: Level,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Print,
    Warn,
    Error,
    Info,
}

thread_local! {
    static LOG: RefCell<Vec<OutputLine>> = const { RefCell::new(Vec::new()) };
}

fn with_log(f: impl FnOnce(&mut Vec<OutputLine>)) {
    LOG.with(|cell| f(&mut cell.borrow_mut()));
}

fn take_log() -> Vec<OutputLine> {
    LOG.with(|cell| std::mem::take(&mut *cell.borrow_mut()))
}

fn format_args(lua: &Lua, args: MultiValue) -> String {
    let mut out = String::new();
    let mut first = true;
    for v in args {
        if !first {
            out.push('\t');
        }
        first = false;
        match value_display(lua, v) {
            Ok(s) => out.push_str(&s),
            Err(_) => out.push_str("<unformattable>"),
        }
    }
    out
}

fn value_display(lua: &Lua, v: Value) -> LuaResult<String> {
    Ok(match v {
        Value::Nil => "nil".to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Integer(i) => i.to_string(),
        Value::Number(n) => {
            if n.fract() == 0.0 && n.is_finite() {
                format!("{}", n as i64)
            } else {
                format!("{n}")
            }
        }
        Value::String(s) => s.to_str().unwrap_or_else(|e| format!("<str: {e}>")),
        other => {
            if let Ok(tostr) = lua.globals().get::<Function>("tostring") {
                if let Ok(s) = tostr.call::<String>(other.clone()) {
                    s
                } else {
                    format!("{other:?}")
                }
            } else {
                format!("{other:?}")
            }
        }
    })
}

fn build_vm() -> LuaResult<Lua> {
    let lua = Lua::new();

    // Strip anything that could touch the host.
    lua.load(
        r#"
        rawset(_G, 'io', nil)
        rawset(_G, 'os', nil)
        rawset(_G, 'loadfile', nil)
        rawset(_G, 'dofile', nil)
        rawset(_G, 'require', nil)
        "#,
    )
    .exec()?;

    let g = lua.globals();

    // print / warn
    g.set(
        "print",
        lua.create_function(|lua, args: MultiValue| {
            with_log(|log| {
                log.push(OutputLine {
                    level: Level::Print,
                    text: format_args(lua, args),
                })
            });
            Ok(())
        })?,
    )?;
    g.set(
        "warn",
        lua.create_function(|lua, args: MultiValue| {
            with_log(|log| {
                log.push(OutputLine {
                    level: Level::Warn,
                    text: format_args(lua, args),
                })
            });
            Ok(())
        })?,
    )?;

    // typeof (respects __type metamethod)
    g.set(
        "typeof",
        lua.create_function(|_lua, v: Value| {
            if let Value::Table(t) = &v {
                if let Some(mt) = t.metatable() {
                    if let Ok(Value::Function(f)) = mt.get::<Value>("__type") {
                        if let Ok(s) = f.call::<String>(v.clone()) {
                            return Ok(s);
                        }
                    }
                }
            }
            Ok(v.type_name().to_string())
        })?,
    )?;

    // task library (synchronous stubs)
    let task = lua.create_table();
    task.set("wait", lua.create_function(|_, _: Variadic<Value>| Ok(0.0f64))?)?;
    task.set(
        "spawn",
        lua.create_function(|_, f: Function| f.call::<()>(()))?,
    )?;
    task.set(
        "defer",
        lua.create_function(|_, f: Function| f.call::<()>(()))?,
    )?;
    task.set(
        "delay",
        lua.create_function(|_, (_t, f): (f64, Function)| f.call::<()>(()))?,
    )?;
    task.set("cancel", lua.create_function(|_, _: Value| Ok(()))?)?;
    g.set("task", task)?;

    install_vector3(&lua)?;
    install_vector2(&lua)?;
    install_color3(&lua)?;
    install_cframe(&lua)?;
    install_udim2(&lua)?;
    install_enum(&lua)?;
    install_instance_stub(&lua)?;
    install_plugin_stub(&lua)?;
    g.set("script", make_instance(&lua, "ModuleScript", "Script")?)?;

    Ok(lua)
}

/// Run script source.
pub fn run_source(source: &str, name: &str) -> RunResult {
    match run_inner(source, name, false) {
        Ok(lines) => RunResult { success: true, lines },
        Err(e) => {
            let mut lines = take_log();
            lines.push(OutputLine {
                level: Level::Error,
                text: e.to_string(),
            });
            RunResult { success: false, lines }
        }
    }
}

/// Run a ModuleScript source and capture its return value.
pub fn run_module(source: &str, name: &str) -> RunResult {
    match run_inner(source, name, true) {
        Ok(lines) => RunResult { success: true, lines },
        Err(e) => {
            let mut lines = take_log();
            lines.push(OutputLine {
                level: Level::Error,
                text: e.to_string(),
            });
            RunResult { success: false, lines }
        }
    }
}

fn run_inner(source: &str, name: &str, is_module: bool) -> LuaResult<Vec<OutputLine>> {
    let lua = build_vm()?;
    if is_module {
        let v: Value = lua.load(source).set_name(name).eval()?;
        let text = format_args(&lua, MultiValue::from_vec(vec![v]));
        with_log(|log| {
            log.push(OutputLine {
                level: Level::Info,
                text: format!("=> {text}"),
            })
        });
    } else {
        lua.load(source).set_name(name).exec()?;
    }
    Ok(take_log())
}

// --------------------------------------------------------------------------
// Minimal Roblox datatypes (plain tables with a metatable).
// --------------------------------------------------------------------------

fn typed_metatable(lua: &Lua, type_name: &str) -> LuaResult<Table> {
    let mt = lua.create_table();
    let name = type_name.to_string();
    mt.set(
        "__type",
        lua.create_function(move |_, _: ()| Ok(name.clone()))?,
    )?;
    Ok(mt)
}

fn simple_tostring(lua: &Lua, fields: &[&str]) -> LuaResult<Function> {
    let fields: Vec<String> = fields.iter().map(|s| s.to_string()).collect();
    lua.create_function(move |_lua, t: Table| {
        let mut parts = Vec::new();
        for f in &fields {
            if let Ok(v) = t.get::<Value>(f.as_str()) {
                parts.push(format!("{v:?}"));
            }
        }
        Ok(parts.join(", "))
    })
}

fn install_vector3(lua: &Lua) -> LuaResult<()> {
    // One shared metatable, cloned into every instance/result.
    let make_mt = || {
        let mt = lua.create_table();
        mt.set("__type", lua.create_function(|_, _: ()| Ok("Vector3"))?)?;
        mt.set(
            "__tostring",
            lua.create_function(|_lua, t: Table| {
                Ok(format!(
                    "{}, {}, {}",
                    t.get::<f64>("X")?,
                    t.get::<f64>("Y")?,
                    t.get::<f64>("Z")?
                ))
            })?,
        )?;
        let wrap = |f: fn(f64, f64, f64, f64, f64, f64) -> (f64, f64, f64)| {
            let mt = mt.clone();
            lua.create_function(move |lua, (a, b): (Table, Table)| {
                let (x, y, z) = f(
                    a.get::<f64>("X")?,
                    a.get::<f64>("Y")?,
                    a.get::<f64>("Z")?,
                    b.get::<f64>("X")?,
                    b.get::<f64>("Y")?,
                    b.get::<f64>("Z")?,
                );
                let t = lua.create_table();
                t.set("X", x)?;
                t.set("Y", y)?;
                t.set("Z", z)?;
                t.set("Magnitude", (x * x + y * y + z * z).sqrt())?;
                t.set_metatable(Some(mt.clone()));
                Ok(t)
            })
        };
        mt.set("__add", wrap(|ax, ay, az, bx, by, bz| (ax + bx, ay + by, az + bz))?)?;
        mt.set("__sub", wrap(|ax, ay, az, bx, by, bz| (ax - bx, ay - by, az - bz))?)?;
        mt.set(
            "__mul",
            {
                let mt2 = mt.clone();
                lua.create_function(move |lua, (a, b): (Value, Value)| {
                    let (x, y, z) = match (&a, &b) {
                        (Value::Table(t), Value::Number(s)) => (
                            t.get::<f64>("X")? * s,
                            t.get::<f64>("Y")? * s,
                            t.get::<f64>("Z")? * s,
                        ),
                        (Value::Number(s), Value::Table(t)) => (
                            t.get::<f64>("X")? * s,
                            t.get::<f64>("Y")? * s,
                            t.get::<f64>("Z")? * s,
                        ),
                        _ => return Err(LuaError::runtime("Vector3 can only be multiplied by a number")),
                    };
                    let out = lua.create_table();
                    out.set("X", x)?;
                    out.set("Y", y)?;
                    out.set("Z", z)?;
                    out.set("Magnitude", (x * x + y * y + z * z).sqrt())?;
                    out.set_metatable(Some(mt2.clone()));
                    Ok(out)
                })?
            },
        )?;
        Ok::<Table, LuaError>(mt)
    };
    let mt = make_mt()?;

    let new_fn = {
        let mt = mt.clone();
        lua.create_function(move |lua, args: Variadic<f64>| {
            let x = args.first().copied().unwrap_or(0.0);
            let y = args.get(1).copied().unwrap_or(0.0);
            let z = args.get(2).copied().unwrap_or(0.0);
            let t = lua.create_table();
            t.set("X", x)?;
            t.set("Y", y)?;
            t.set("Z", z)?;
            t.set("Magnitude", (x * x + y * y + z * z).sqrt())?;
            t.set_metatable(Some(mt.clone()));
            Ok(t)
        })?
    };
    let v3 = lua.create_table();
    v3.set("new", new_fn.clone())?;
    let zero = new_fn.call::<Table>((0.0, 0.0, 0.0))?;
    let one = new_fn.call::<Table>((1.0, 1.0, 1.0))?;
    v3.set("zero", zero)?;
    v3.set("one", one)?;
    lua.globals().set("Vector3", v3)?;
    Ok(())
}

fn install_vector2(lua: &Lua) -> LuaResult<()> {
    let v2 = lua.create_table();
    v2.set(
        "new",
        lua.create_function(|lua, (x, y): (f64, f64)| {
            let t = lua.create_table();
            t.set("X", x)?;
            t.set("Y", y)?;
            t.set("Magnitude", (x * x + y * y).sqrt())?;
            t.set_metatable(Some(typed_metatable(lua, "Vector2")?));
            Ok(t)
        })?,
    )?;
    lua.globals().set("Vector2", v2)?;
    Ok(())
}

fn install_color3(lua: &Lua) -> LuaResult<()> {
    let c3 = lua.create_table();
    c3.set(
        "new",
        lua.create_function(|lua, (r, g, b): (f64, f64, f64)| {
            let t = lua.create_table();
            t.set("R", r)?;
            t.set("G", g)?;
            t.set("B", b)?;
            t.set_metatable(Some(typed_metatable(lua, "Color3")?));
            Ok(t)
        })?,
    )?;
    c3.set(
        "fromRGB",
        lua.create_function(|lua, (r, g, b): (i64, i64, i64)| {
            let t = lua.create_table();
            t.set("R", r as f64 / 255.0)?;
            t.set("G", g as f64 / 255.0)?;
            t.set("B", b as f64 / 255.0)?;
            t.set_metatable(Some(typed_metatable(lua, "Color3")?));
            Ok(t)
        })?,
    )?;
    c3.set(
        "fromHSV",
        lua.create_function(|lua, (_h, _s, _v): (f64, f64, f64)| {
            // No HSV conversion in this minimal sandbox; return white.
            let t = lua.create_table();
            t.set("R", 1.0f64)?;
            t.set("G", 1.0f64)?;
            t.set("B", 1.0f64)?;
            t.set_metatable(Some(typed_metatable(lua, "Color3")?));
            Ok(t)
        })?,
    )?;
    lua.globals().set("Color3", c3)?;
    Ok(())
}

fn install_cframe(lua: &Lua) -> LuaResult<()> {
    let cf = lua.create_table();
    cf.set(
        "new",
        lua.create_function(|lua, (x, y, z): (f64, f64, f64)| {
            let t = lua.create_table();
            t.set("X", x)?;
            t.set("Y", y)?;
            t.set("Z", z)?;
            let p = lua.create_table();
            p.set("X", x)?;
            p.set("Y", y)?;
            p.set("Z", z)?;
            t.set("Position", p)?;
            t.set_metatable(Some(typed_metatable(lua, "CFrame")?));
            Ok(t)
        })?,
    )?;
    cf.set(
        "Angles",
        lua.create_function(|lua, (rx, ry, rz): (f64, f64, f64)| {
            let t = lua.create_table();
            t.set("RX", rx)?;
            t.set("RY", ry)?;
            t.set("RZ", rz)?;
            t.set_metatable(Some(typed_metatable(lua, "CFrame")?));
            Ok(t)
        })?,
    )?;
    lua.globals().set("CFrame", cf)?;
    Ok(())
}

fn install_udim2(lua: &Lua) -> LuaResult<()> {
    let ud = lua.create_table();
    ud.set(
        "new",
        lua.create_function(|lua, (xs, xo, ys, yo): (f64, i64, f64, i64)| {
            let t = lua.create_table();
            t.set("XScale", xs)?;
            t.set("XOffset", xo)?;
            t.set("YScale", ys)?;
            t.set("YOffset", yo)?;
            t.set_metatable(Some(typed_metatable(lua, "UDim2")?));
            Ok(t)
        })?,
    )?;
    ud.set(
        "fromScale",
        lua.create_function(|lua, (x, y): (f64, f64)| {
            let t = lua.create_table();
            t.set("XScale", x)?;
            t.set("XOffset", 0i64)?;
            t.set("YScale", y)?;
            t.set("YOffset", 0i64)?;
            t.set_metatable(Some(typed_metatable(lua, "UDim2")?));
            Ok(t)
        })?,
    )?;
    lua.globals().set("UDim2", ud)?;
    Ok(())
}

fn install_enum(lua: &Lua) -> LuaResult<()> {
    // Permissive: Enum.Material.Plastic returns a table {Name, Value=0,
    // EnumType}. Numeric values don't match real Roblox — good enough for
    // logic tests that branch on enum *names*.
    let item_mt = lua.create_table();
    item_mt.set(
        "__index",
        lua.create_function(|lua, (_, key): (Table, String)| {
            let t = lua.create_table();
            t.set("Name", key.clone())?;
            t.set("Value", 0i64)?;
            t.set("EnumType", key)?;
            Ok(t)
        })?,
    )?;
    let group_mt = lua.create_table();
    group_mt.set(
        "__index",
        lua.create_function(move |lua, (_t, _key): (Table, String)| {
            let group = lua.create_table();
            group.set_metatable(Some(item_mt.clone()));
            Ok(group)
        })?,
    )?;
    let enum_root = lua.create_table();
    enum_root.set_metatable(Some(group_mt));
    lua.globals().set("Enum", enum_root)?;
    Ok(())
}

fn make_instance(lua: &Lua, class: &str, name: &str) -> LuaResult<Table> {
    let t = lua.create_table();
    t.set("Name", name)?;
    t.set("ClassName", class)?;
    let noop = lua.create_function(|_, _: Variadic<Value>| Ok(Variadic::<Value>::new()))?;
    for m in [
        "GetChildren",
        "GetDescendants",
        "FindFirstChild",
        "WaitForChild",
        "GetActor",
        "Clone",
        "Destroy",
        "GetPropertyChangedSignal",
        "GetAttribute",
        "SetAttribute",
    ] {
        t.set(m, noop.clone())?;
    }
    let class_name = class.to_string();
    let isa = lua.create_function(move |_, (_self, name): (Table, String)| Ok(name == class_name))?;
    t.set("IsA", isa)?;
    let mt = typed_metatable(lua, "Instance")?;
    let _ = t.set_metatable(Some(mt));
    Ok(t)
}

fn install_instance_stub(lua: &Lua) -> LuaResult<()> {
    let inst = lua.create_table();
    let new_fn = lua.create_function(|lua, (class, name): (String, Option<String>)| {
        let name = name.unwrap_or_else(|| class.clone());
        make_instance(lua, &class, &name)
    })?;
    inst.set("new", new_fn)?;
    lua.globals().set("Instance", inst)?;
    Ok(())
}

fn install_plugin_stub(lua: &Lua) -> LuaResult<()> {
    let button = lua.create_table();
    let noop = lua.create_function(|_, _: Variadic<Value>| Ok(Variadic::<Value>::new()))?;
    for m in ["Click", "SetActive", "SetEnabled"] {
        button.set(m, noop.clone())?;
    }

    let toolbar = lua.create_table();
    let btn = button.clone();
    // Called as tb:CreateButton(...) — first arg is the toolbar table itself.
    let create_btn = lua.create_function(move |_lua, (_self, _args): (Table, Variadic<Value>)| {
        Ok(btn.clone())
    })?;
    toolbar.set("CreateButton", create_btn)?;

    let plugin = lua.create_table();
    let tb = toolbar.clone();
    let create_toolbar = lua.create_function(move |_lua, (_self, name): (Table, String)| {
        let tb = tb.clone();
        tb.set("_name", name)?;
        Ok(tb)
    })?;
    plugin.set("CreateToolbar", create_toolbar)?;
    for m in [
        "GetMouse",
        "OpenWikiPage",
        "Activate",
        "Deactivate",
        "ImportFbxRbx",
        "StartDrag",
        "CreatePluginMenu",
        "OpenView",
    ] {
        plugin.set(m, noop.clone())?;
    }
    let create_dock = lua.create_function(|lua, (_self, name, _info): (Table, String, Value)| {
        make_instance(lua, "DockWidgetPluginGui", &name)
    })?;
    plugin.set("CreateDockWidgetPluginGui", create_dock)?;
    // plugin:GetSetting / SetSetting need a backing table — give them a trivial
    // in-memory settings map so plugins that persist prefs don't crash.
    let settings_store = lua.create_table();
    let get_setting = {
        let store = settings_store.clone();
        lua.create_function(move |_lua, (_self, key): (Table, String)| {
            Ok(store.get::<Value>(key).unwrap_or(Value::Nil))
        })?
    };
    let set_setting = {
        let store = settings_store.clone();
        lua.create_function(move |_lua, (_self, key, value): (Table, String, Value)| {
            store.set(key, value)?;
            Ok(())
        })?
    };
    plugin.set("GetSetting", get_setting)?;
    plugin.set("SetSetting", set_setting)?;
    lua.globals().set("plugin", plugin)?;
    // Unhide plugin menus / Studio-only globals that plugins frequently read.
    let settings = lua.create_table();
    settings.set(
        "GetService",
        lua.create_function(|lua, name: String| match name.as_str() {
            "GameSettings" => {
                let gs = lua.create_table();
                gs.set(
                    "IsFullscreen",
                    lua.create_function(|_, ()| Ok(false))?,
                )?;
                gs.set(
                    "InStudio",
                    lua.create_function(|_, ()| Ok(true))?,
                )?;
                Ok(gs)
            }
            _ => Ok(lua.create_table()),
        })?,
    )?;
    lua.globals().set("settings", settings)?;
    lua.globals().set("UserSettings", lua.create_table())?;
    lua.globals().set("GameSettings", lua.create_table())?;
    Ok(())
}

// ==========================================================================
// Command-bar mode: bind Instance.new / game / workspace to a real WeakDom so
// snippets actually create and modify objects in the loaded place.
// ==========================================================================

use std::rc::Rc;
use rbx_dom_weak::{
    types::{Ref as DomRef, Variant as DomVariant},
    InstanceBuilder, WeakDom,
};

/// Summary returned by the command bar so the editor can refresh explorer/3D.
#[derive(Debug, Clone, Default)]
pub struct CommandOutcome {
    pub created: Vec<DomRef>,
    pub destroyed: Vec<DomRef>,
    pub mutated: usize,
    /// The selection after the command ran (from Selection service).
    pub selected: Vec<DomRef>,
    /// Set if the command requested an undo/redo, so the editor can pop its
    /// own history stack as well.
    pub undo: bool,
    pub redo: bool,
}

/// Mutable per-run state that also emulates Studio services that aren't part
/// of the DOM itself (Selection, ChangeHistoryService).
struct CommandState {
    dom: Rc<RefCell<WeakDom>>,
    cache: std::collections::HashMap<DomRef, Table>,
    services: std::collections::HashMap<String, DomRef>,
    created: Vec<DomRef>,
    /// Virtual Selection service contents (ordered, like Studio).
    selection: Vec<DomRef>,
    /// Undo/redo stacks of serialized place snapshots.
    undo_stack: Vec<Vec<u8>>,
    redo_stack: Vec<Vec<u8>>,
    /// When a waypoint is set, we snapshot *before* the next mutation.
    waypoint_open: bool,
}

thread_local! {
    static COMMAND_OUTCOME: RefCell<CommandOutcome> = const { RefCell::new(CommandOutcome {
        created: Vec::new(),
        destroyed: Vec::new(),
        mutated: 0,
        selected: Vec::new(),
        undo: false,
        redo: false,
    }) };
    /// Persisted undo/redo snapshots for ChangeHistoryService.
    static UNDO_STACK: RefCell<Vec<Vec<u8>>> = const { RefCell::new(Vec::new()) };
    static REDO_STACK: RefCell<Vec<Vec<u8>>> = const { RefCell::new(Vec::new()) };
}

/// Run a snippet against a real, mutable DataModel. The snippet can use the
/// standard globals plus `game`, `workspace`, `Instance.new`, `GetService`,
/// property get/set, `:Clone()`, `:Destroy()`, `:FindFirstChild()`, and
/// `:GetChildren()`.
pub fn run_command(dom_rc: Rc<RefCell<WeakDom>>, source: &str, name: &str) -> Result<CommandOutcome, String> {
    LOG.with(|c| c.borrow_mut().clear());
    COMMAND_OUTCOME.with(|c| *c.borrow_mut() = CommandOutcome::default());

    let lua = build_vm().map_err(|e| e.to_string())?;

    // Replace the stub `Instance.new` with the real one and install game.
    let selection = install_command_globals(&lua, dom_rc).map_err(|e| e.to_string())?;

    match lua.load(source).set_name(name).exec() {
        Ok(()) => {
            let mut outcome = COMMAND_OUTCOME.with(|c| c.borrow().clone());
            outcome.selected = selection.borrow().clone();
            Ok(outcome)
        }
        Err(e) => {
            let mut lines = take_log();
            lines.push(OutputLine { level: Level::Error, text: e.to_string() });
            LOG.with(|c| *c.borrow_mut() = lines);
            Err(e.to_string())
        }
    }
}

/// Drain output produced by the most recent `run_command`.
pub fn take_command_log() -> Vec<OutputLine> { take_log() }

/// Clear the persisted undo/redo stacks. Call when opening a new place so
/// undo from a previous document doesn't affect the current one.
pub fn reset_command_history() {
    UNDO_STACK.with(|u| u.borrow_mut().clear());
    REDO_STACK.with(|r| r.borrow_mut().clear());
}

fn install_command_globals(lua: &Lua, dom_rc: Rc<RefCell<WeakDom>>) -> LuaResult<Rc<RefCell<Vec<DomRef>>>> {
    // A handle to a DOM instance is a plain table with a single numeric
    // field "_ref" holding the i64 low-64-bits of the Ref. We keep a
    // per-VM cache so the same Ref always maps to one table (important for
    // `==` and Parent cycles).
    let cache: Rc<RefCell<std::collections::HashMap<DomRef, Table>>> =
        Rc::new(RefCell::new(std::collections::HashMap::new()));
    let instance_mt = make_instance_metatable(lua, dom_rc.clone(), cache.clone())?;

    let root_ref = dom_rc.borrow().root_ref();
    let game_table = ref_to_table(lua, dom_rc.clone(), cache.clone(), instance_mt.clone(), root_ref)?;
    lua.globals().set("game", game_table.clone())?;
    lua.globals().set("Game", game_table.clone())?;

    // Resolve Workspace eagerly; create it if missing.
    let ws = ensure_service(lua, dom_rc.clone(), cache.clone(), instance_mt.clone(), "Workspace")?;
    lua.globals().set("workspace", ws.clone())?;
    lua.globals().set("Workspace", ws)?;

    // --- Selection service (virtual; mirrors Studio's API surface) ---
    let selection: Rc<RefCell<Vec<DomRef>>> = Rc::new(RefCell::new(Vec::new()));
    let sel_get = {
        let sel = selection.clone();
        let dom = dom_rc.clone();
        let cache = cache.clone();
        let mt = instance_mt.clone();
        lua.create_function(move |lua, _this: Table| {
            let tables: Vec<Table> = sel.borrow()
                .iter()
                .filter_map(|r| {
                    if dom.borrow().get_by_ref(*r).is_some() {
                        ref_to_table(lua, dom.clone(), cache.clone(), mt.clone(), *r).ok()
                    } else { None }
                })
                .collect();
            Ok(tables)
        })?
    };
    let sel_set = {
        let sel = selection.clone();
        lua.create_function(move |lua, (_this, items): (Table, Value)| {
            // Selection:Set accepts an array table of Instances (Studio API),
            // but we also tolerate being passed instances as varargs.
            let mut refs = Vec::new();
            let mut push_val = |v: Value| -> LuaResult<()> {
                if let Value::Table(t) = v {
                    if let Some(r) = table_to_ref(&t)? { refs.push(r); }
                }
                Ok(())
            };
            match items {
                Value::Table(t) => {
                    // Could be an array of Instances or a single Instance.
                    if let Some(r) = table_to_ref(&t)? {
                        refs.push(r);
                    } else {
                        // Iterate the array part manually (sequence_values'
                        // exact API varies across luaur versions).
                        let len: usize = t.len()?;
                        for i in 1..=len as i64 {
                            if let Ok(v) = t.get::<Value>(i) {
                                push_val(v)?;
                            }
                        }
                    }
                }
                other => push_val(other)?,
            }
            *sel.borrow_mut() = refs;
            Ok(())
        })?
    };
    let sel_add = {
        let sel = selection.clone();
        lua.create_function(move |lua, (_this, item): (Table, Table)| {
            if let Some(r) = table_to_ref(&item)? {
                let mut s = sel.borrow_mut();
                if !s.contains(&r) { s.push(r); }
            }
            Ok(())
        })?
    };
    let sel_remove = {
        let sel = selection.clone();
        lua.create_function(move |lua, (_this, item): (Table, Table)| {
            if let Some(r) = table_to_ref(&item)? {
                sel.borrow_mut().retain(|x| *x != r);
            }
            Ok(())
        })?
    };
    let sel_clear = {
        let sel = selection.clone();
        lua.create_function(move |_, _this: Table| { sel.borrow_mut().clear(); Ok(()) })?
    };
    let selection_svc = lua.create_table();
    selection_svc.set("Get", sel_get)?;
    selection_svc.set("Set", sel_set)?;
    selection_svc.set("Add", sel_add)?;
    selection_svc.set("Remove", sel_remove)?;
    selection_svc.set("Clear", sel_clear)?;
    lua.globals().set("Selection", selection_svc)?;

    // --- ChangeHistoryService (snapshot-based undo/redo) ---
    // Stacks live in module-scope thread-locals so they persist across
    // commands (each command gets a fresh VM). `reset_command_history()`
    // clears them when a new place is opened.
    let dom_rc2 = dom_rc.clone();
    let snapshot = move || -> LuaResult<Vec<u8>> {
        let d = dom_rc2.borrow();
        let root = d.root_ref();
        let mut buf = Vec::new();
        rbx_binary::to_writer(&mut buf, &d, &[root])
            .map_err(|e| LuaError::runtime(format!("snapshot failed: {e}")))?;
        Ok(buf)
    };
    let dom_rc3 = dom_rc.clone();
    let restore = move |bytes: &[u8]| -> LuaResult<()> {
        let restored = rbx_binary::from_reader(bytes)
            .map_err(|e| LuaError::runtime(format!("restore failed: {e}")))?;
        *dom_rc3.borrow_mut() = restored;
        Ok(())
    };
    // Seed with the starting state on first use if the stack is empty.
    let needs_seed = UNDO_STACK.with(|u| u.borrow().is_empty());
    if needs_seed {
        if let Ok(start) = snapshot() {
            UNDO_STACK.with(|u| u.borrow_mut().push(start));
        }
    }
    let snap1 = snapshot.clone();
    let set_waypoint = lua.create_function(
        move |_, (_this, _name, _opts): (Table, String, Option<Value>)| {
            if let Ok(snap) = snap1() {
                UNDO_STACK.with(|u| u.borrow_mut().push(snap));
                REDO_STACK.with(|r| r.borrow_mut().clear());
            }
            Ok(())
        },
    )?;
    let restore_undo = restore.clone();
    let sel_undo = selection.clone();
    let undo_fn = lua.create_function(move |_, _this: Table| {
        let mut did = false;
        UNDO_STACK.with(|u| {
            let mut us = u.borrow_mut();
            if us.len() > 1 {
                if let Some(current) = us.pop() {
                    REDO_STACK.with(|r| r.borrow_mut().push(current));
                    if let Some(prev) = us.last() {
                        let _ = restore_undo(prev);
                        sel_undo.borrow_mut().clear();
                        COMMAND_OUTCOME.with(|o| o.borrow_mut().undo = true);
                        did = true;
                    }
                }
            }
        });
        Ok(did)
    })?;
    let restore_redo = restore.clone();
    let sel_redo = selection.clone();
    let redo_fn = lua.create_function(move |_, _this: Table| {
        let mut did = false;
        REDO_STACK.with(|r| {
            if let Some(next) = r.borrow_mut().pop() {
                UNDO_STACK.with(|u| u.borrow_mut().push(next.clone()));
                let _ = restore_redo(&next);
                sel_redo.borrow_mut().clear();
                COMMAND_OUTCOME.with(|o| o.borrow_mut().redo = true);
                did = true;
            }
        });
        Ok(did)
    })?;
    let snap_reset = snapshot.clone();
    let reset_history = lua.create_function(move |_, _this: Table| {
        UNDO_STACK.with(|u| u.borrow_mut().clear());
        REDO_STACK.with(|r| r.borrow_mut().clear());
        if let Ok(s) = snap_reset() {
            UNDO_STACK.with(|u| u.borrow_mut().push(s));
        }
        Ok(())
    })?;
    let chs = lua.create_table();
    chs.set("TryBeginRecording", set_waypoint.clone())?;
    chs.set("FinishRecording", set_waypoint.clone())?;
    chs.set("SetWaypoint", set_waypoint)?;
    chs.set("Undo", undo_fn)?;
    chs.set("Redo", redo_fn)?;
    chs.set("ResetHistory", reset_history)?;
    chs.set("GetCanUndo", lua.create_function(|_, _this: Table| {
        Ok(UNDO_STACK.with(|u| u.borrow().len() > 1))
    })?)?;
    chs.set("GetCanRedo", lua.create_function(|_, _this: Table| {
        Ok(REDO_STACK.with(|r| !r.borrow().is_empty()))
    })?)?;
    lua.globals().set("ChangeHistoryService", chs)?;

    // Instance.new(class, [parent])
    let inst_new = {
        let dom = dom_rc.clone();
        let cache = cache.clone();
        let mt = instance_mt.clone();
        lua.create_function(move |lua, (class, parent): (String, Option<Table>)| {
            let parent_ref = parent
                .as_ref()
                .map(|p| table_to_ref(p))
                .transpose()?
                .flatten()
                .unwrap_or_else(|| dom.borrow().root_ref());
            let r = {
                let mut d = dom.borrow_mut();
                d.insert(parent_ref, InstanceBuilder::new(class.clone()))
            };
            COMMAND_OUTCOME.with(|o| o.borrow_mut().created.push(r));
            ref_to_table(lua, dom.clone(), cache.clone(), mt.clone(), r)
        })?
    };
    let inst_table = lua.create_table();
    inst_table.set("new", inst_new)?;
    lua.globals().set("Instance", inst_table)?;
    Ok(selection)
}

fn make_instance_metatable(
    lua: &Lua,
    dom: Rc<RefCell<WeakDom>>,
    cache: Rc<RefCell<std::collections::HashMap<DomRef, Table>>>,
) -> LuaResult<Rc<Table>> {
    let mt = lua.create_table();

    // __index: methods first, then Name/ClassName/Parent, then properties,
    // then children by name.
    let mt_handle: Rc<RefCell<Option<Rc<Table>>>> = Rc::new(RefCell::new(None));
    let index = {
        let dom = dom.clone();
        let cache = cache.clone();
        let mt_handle = mt_handle.clone();
        lua.create_function(move |lua, (this, key): (Table, String)| {
            if let Some(f) = method_for(lua, dom.clone(), cache.clone(), mt_handle.borrow().as_ref().unwrap().clone(), &key)? {
                return Ok(Value::Function(f));
            }
            let Some(r) = table_to_ref(&this)? else { return Ok(Value::Nil) };
            let d = dom.borrow();
            let Some(inst) = d.get_by_ref(r) else { return Ok(Value::Nil) };
            let v = match key.as_str() {
                "Name" => Value::String(lua.create_string(&inst.name)),
                "ClassName" => Value::String(lua.create_string(&inst.class)),
                "Parent" => {
                    let p = inst.parent();
                    if p.is_none() { Value::Nil } else {
                        Value::Table(ref_to_table(lua, dom.clone(), cache.clone(), mt_handle.borrow().as_ref().unwrap().clone(), p)?)
                    }
                },
                _ => {
                    if let Some(prop) = inst.properties.get(&rbx_dom_weak::Ustr::from(key.as_str())) {
                        variant_to_value(lua, prop)?
                    } else {
                        let mut found = None;
                        for &c in inst.children() {
                            if d.get_by_ref(c).is_some_and(|i| i.name == key) { found = Some(c); break; }
                        }
                        match found {
                            Some(c) => Value::Table(ref_to_table(lua, dom.clone(), cache.clone(), mt_handle.borrow().as_ref().unwrap().clone(), c)?),
                            None => Value::Nil,
                        }
                    }
                }
            };
            Ok(v)
        })?
    };
    mt.set("__index", index)?;

    // __newindex: Name, Parent, and arbitrary properties.
    let newindex = {
        let dom = dom.clone();
        lua.create_function(move |lua, (this, key, value): (Table, String, Value)| {
            let Some(r) = table_to_ref(&this)? else { return Ok(()) };
            match key.as_str() {
                "Name" => if let Value::String(s) = value {
                    if let Ok(mut d) = dom.try_borrow_mut() { if let Some(i) = d.get_by_ref_mut(r) { i.name = s.to_str()?; } }
                },
                "ClassName" => {} // read-only
                "Parent" => {
                    let new_parent = match value {
                        Value::Table(t) => table_to_ref(&t)?.unwrap_or(dom.borrow().root_ref()),
                        Value::Nil => dom.borrow().root_ref(),
                        _ => return Err(LuaError::runtime("Parent must be an Instance or nil")),
                    };
                    dom.borrow_mut().transfer_within(r, new_parent);
                }
                _ => if let Some(variant) = value_to_variant(lua, &value)? {
                    if let Ok(mut d) = dom.try_borrow_mut() {
                        if let Some(i) = d.get_by_ref_mut(r) {
                            i.properties.insert(rbx_dom_weak::Ustr::from(&key), variant);
                            COMMAND_OUTCOME.with(|o| o.borrow_mut().mutated += 1);
                        }
                    }
                }
            }
            Ok(())
        })?
    };
    mt.set("__newindex", newindex)?;
    mt.set(
        "__tostring",
        lua.create_function(|_, this: Table| {
            let n: String = this.raw_get("_name")?;
            let c: String = this.raw_get("_class")?;
            Ok(format!("{n} ({c})"))
        })?,
    )?;
    let rc_mt = Rc::new(mt);
    *mt_handle.borrow_mut() = Some(rc_mt.clone());
    Ok(rc_mt)
}

fn method_for(
    lua: &Lua,
    dom: Rc<RefCell<WeakDom>>,
    cache: Rc<RefCell<std::collections::HashMap<DomRef, Table>>>,
    mt: Rc<Table>,
    name: &str,
) -> LuaResult<Option<Function>> {
    let d = dom.clone();
    let c = cache.clone();
    let f = match name {
        "GetService" => Some(lua.create_function(move |lua, (_this, name): (Table, String)| {
            // Virtual (non-DOM) services are exposed as globals.
            match name.as_str() {
                "Selection" | "ChangeHistoryService" | "CoreGui" | "PluginGuiService"
                | "UserInputService" | "RunService" | "HttpService" | "MarketplaceService"
                | "Players" | "Lighting" | "ReplicatedStorage" | "ServerStorage"
                | "ServerScriptService" | "StarterGui" | "StarterPack" | "StarterPlayer"
                | "SoundService" | "TweenService" => {
                    if let Ok(svc) = lua.globals().get::<Value>(&name) {
                        if !svc.is_nil() { return Ok(svc); }
                    }
                }
                _ => {}
            }
            let t = ensure_service(lua, d.clone(), c.clone(), mt.clone(), &name)?;
            Ok(Value::Table(t))
        })?),
        "FindFirstChild" => Some(lua.create_function(move |lua, (this, name): (Table, String)| {
            let Some(r) = table_to_ref(&this)? else { return Ok(Value::Nil) };
            let found = {
                let d = d.borrow();
                let inst = d.get_by_ref(r);
                inst.and_then(|i| i.children().iter().copied().find(|c| d.get_by_ref(*c).is_some_and(|i| i.name == name)))
            };
            Ok(match found {
                Some(r) => Value::Table(ref_to_table(lua, d.clone(), c.clone(), mt.clone(), r)?),
                None => Value::Nil,
            })
        })?),
        "GetChildren" => Some(lua.create_function(move |lua, this: Table| {
            let Some(r) = table_to_ref(&this)? else { return Ok(Vec::<Value>::new()) };
            let children = d.borrow().get_by_ref(r).map(|i| i.children().to_vec()).unwrap_or_default();
            let mut out = Vec::with_capacity(children.len());
            for ch in children {
                out.push(Value::Table(ref_to_table(lua, d.clone(), c.clone(), mt.clone(), ch)?));
            }
            Ok(out)
        })?),
        "IsA" => Some(lua.create_function(move |_lua, (this, class): (Table, String)| {
            let Some(r) = table_to_ref(&this)? else { return Ok(false) };
            Ok(d.borrow().get_by_ref(r).is_some_and(|i| i.class == class))
        })?),
        "Clone" => Some(lua.create_function(move |lua, this: Table| {
            let Some(r) = table_to_ref(&this)? else { return Err(LuaError::runtime("cannot clone <destroyed>")) };
            let new_ref = {
                let d = d.borrow();
                let root = d.root_ref();
                let builder = clone_tree(&d, r);
                drop(d);
                dom.borrow_mut().insert(root, builder)
            };
            COMMAND_OUTCOME.with(|o| o.borrow_mut().created.push(new_ref));
            ref_to_table(lua, d.clone(), c.clone(), mt.clone(), new_ref)
        })?),
        "Destroy" => Some(lua.create_function(move |_lua, this: Table| {
            if let Ok(Some(r)) = table_to_ref(&this) {
                d.borrow_mut().destroy(r);
                COMMAND_OUTCOME.with(|o| o.borrow_mut().destroyed.push(r));
            }
            Ok(())
        })?),
        "GetFullName" => Some(lua.create_function(move |_lua, this: Table| {
            let Some(r) = table_to_ref(&this)? else { return Ok(String::new()) };
            let d = d.borrow();
            let mut parts = Vec::new();
            let mut cur = Some(r);
            while let Some(x) = cur {
                if x == d.root_ref() { break; }
                let Some(inst) = d.get_by_ref(x) else { break };
                parts.push(inst.name.clone());
                cur = if inst.parent().is_none() { None } else { Some(inst.parent()) };
            }
            parts.reverse();
            Ok(parts.join("."))
        })?),
        _ => None,
    };
    Ok(f)
}

fn ensure_service(
    lua: &Lua,
    dom: Rc<RefCell<WeakDom>>,
    cache: Rc<RefCell<std::collections::HashMap<DomRef, Table>>>,
    mt: Rc<Table>,
    name: &str,
) -> LuaResult<Table> {
    let root = dom.borrow().root_ref();
    let existing = dom.borrow().get_by_ref(root).and_then(|root_inst| {
        root_inst.children().iter().copied().find(|c| {
            dom.borrow().get_by_ref(*c).is_some_and(|i| i.class == name || i.name == name)
        })
    });
    let r = match existing {
        Some(r) => r,
        None => {
            let b = InstanceBuilder::new(name).with_name(name);
            dom.borrow_mut().insert(root, b)
        }
    };
    ref_to_table(lua, dom, cache, mt, r)
}

thread_local! {
    static REF_TABLE: RefCell<std::collections::HashMap<i64, DomRef>> = RefCell::new(std::collections::HashMap::new());
}
fn table_to_ref(t: &Table) -> LuaResult<Option<DomRef>> {
    Ok(match t.raw_get::<Value>("_ref")? {
        Value::Integer(i) => REF_TABLE.with(|m| m.borrow().get(&(i as i64)).copied()),
        _ => None,
    })
}

fn ref_to_table(
    lua: &Lua,
    dom: Rc<RefCell<WeakDom>>,
    cache: Rc<RefCell<std::collections::HashMap<DomRef, Table>>>,
    mt: Rc<Table>,
    r: DomRef,
) -> LuaResult<Table> {
    if let Some(t) = cache.borrow().get(&r) {
        return Ok(t.clone());
    }
    let (name, class) = {
        let d = dom.borrow();
        match d.get_by_ref(r) {
            Some(i) => (i.name.clone(), i.class.clone()),
            None => ("<destroyed>".into(), "<<<null>>".into()),
        }
    };
    let t = lua.create_table();
    let id = ref_to_i64(r);
    t.raw_set("_ref", id)?;
    REF_TABLE.with(|m| m.borrow_mut().insert(id, r));
    t.raw_set("_name", name)?;
    t.raw_set("_class", class.as_str())?;
    let _ = t.set_metatable(Some((*mt).clone()));
    cache.borrow_mut().insert(r, t.clone());
    Ok(t)
}

fn ref_to_i64(r: DomRef) -> i64 {
    use std::sync::atomic::{AtomicI64, Ordering};
    static COUNTER: AtomicI64 = AtomicI64::new(1);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    REF_TABLE.with(|m| m.borrow_mut().insert(id, r));
    id
}

fn clone_tree(dom: &WeakDom, r: DomRef) -> InstanceBuilder {
    let inst = dom.get_by_ref(r).expect("clone: source vanished");
    let mut b = InstanceBuilder::new(inst.class.clone()).with_name(inst.name.clone());
    for (k, v) in &inst.properties {
        b = b.with_property(k.clone(), v.clone());
    }
    for &c in inst.children() {
        b = b.with_child(clone_tree(dom, c));
    }
    b
}

fn variant_to_value(lua: &Lua, v: &DomVariant) -> LuaResult<Value> {
    use rbx_dom_weak::types::Variant;
    Ok(match v {
        Variant::String(s) => Value::String(lua.create_string(s)),
        Variant::Bool(b) => Value::Boolean(*b),
        Variant::Float32(n) => Value::Number(*n as f64),
        Variant::Float64(n) => Value::Number(*n),
        Variant::Int32(n) => Value::Number(*n as f64),
        Variant::Int64(n) => Value::Number(*n as f64),
        Variant::Vector3(v) => {
            let t = lua.create_table();
            t.set("X", v.x as f64)?; t.set("Y", v.y as f64)?; t.set("Z", v.z as f64)?;
            Value::Table(t)
        }
        Variant::Color3(c) => {
            let t = lua.create_table();
            t.set("R", c.r as f64)?; t.set("G", c.g as f64)?; t.set("B", c.b as f64)?;
            Value::Table(t)
        }
        Variant::Enum(e) => Value::Number(e.to_u32() as f64),
        _ => Value::Nil,
    })
}

fn value_to_variant(_lua: &Lua, v: &Value) -> LuaResult<Option<DomVariant>> {
    use rbx_dom_weak::types as ty;
    Ok(match v {
        Value::String(s) => Some(DomVariant::String(s.to_str()?.to_string())),
        Value::Boolean(b) => Some(DomVariant::Bool(*b)),
        Value::Integer(i) => Some(DomVariant::Int64(*i)),
        Value::Number(n) => Some(DomVariant::Float64(*n)),
        Value::Table(t) => {
            let has = |k: &str| t.get::<Value>(k).is_ok();
            if has("R") && has("G") && has("B") {
                Some(DomVariant::Color3(ty::Color3::new(
                    t.get::<f64>("R")? as f32,
                    t.get::<f64>("G")? as f32,
                    t.get::<f64>("B")? as f32,
                )))
            } else if has("X") && has("Z") {
                Some(DomVariant::Vector3(ty::Vector3::new(
                    t.get::<f64>("X")? as f32,
                    t.get::<f64>("Y")? as f32,
                    t.get::<f64>("Z")? as f32,
                )))
            } else {
                None
            }
        }
        _ => None,
    })
}
