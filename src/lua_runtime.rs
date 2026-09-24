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
use std::cell::{Cell, RefCell};

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
    install_tween_info(&lua)?;
    install_enum(&lua)?;
    install_instance_stub(&lua)?;
    install_plugin_stub(&lua)?;
    g.set("script", make_instance(&lua, "ModuleScript", "Script")?)?;

    Ok(lua)
}

/// A parser diagnostic produced without executing the script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxDiagnostic {
    pub line: Option<usize>,
    pub message: String,
}

/// Compile Luau source to bytecode without executing it. This powers live
/// editor diagnostics while keeping game/plugin code completely sandboxed.
pub fn check_syntax(source: &str, name: &str) -> Vec<SyntaxDiagnostic> {
    let lua = Lua::new();
    match lua.load(source).set_name(name).into_function() {
        Ok(_) => Vec::new(),
        Err(error) => {
            let raw = error.to_string();
            vec![SyntaxDiagnostic {
                line: diagnostic_line(&raw),
                message: raw,
            }]
        }
    }
}

fn diagnostic_line(message: &str) -> Option<usize> {
    // luaur errors commonly contain `chunk:12:` or `[string "chunk"]:12:`.
    message.split(':').find_map(|part| part.trim().parse::<usize>().ok())
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

fn install_tween_info(lua: &Lua) -> LuaResult<()> {
    let tween_info=lua.create_table();
    tween_info.set("new",lua.create_function(|lua,args:Variadic<Value>|{
        let info=lua.create_table();
        let number=|index:usize,fallback:f64|match args.get(index){Some(Value::Number(value))=>*value,Some(Value::Integer(value))=>*value as f64,_=>fallback};
        info.set("Time",number(0,1.0))?; info.set("EasingStyle",args.get(1).cloned().unwrap_or(Value::Nil))?;
        info.set("EasingDirection",args.get(2).cloned().unwrap_or(Value::Nil))?; info.set("RepeatCount",number(3,0.0) as i64)?;
        info.set("Reverses",matches!(args.get(4),Some(Value::Boolean(true))))?; info.set("DelayTime",number(5,0.0))?;
        info.set_metatable(Some(typed_metatable(lua,"TweenInfo")?)); Ok(info)
    })?)?;
    lua.globals().set("TweenInfo",tween_info)
}

fn install_enum(lua: &Lua) -> LuaResult<()> {
    let root_mt=lua.create_table();
    root_mt.set("__index",lua.create_function(|lua,(_root,enum_type):(Table,String)|{
        let group=lua.create_table();let item_mt=lua.create_table();let captured=enum_type.clone();
        item_mt.set("__index",lua.create_function(move |lua,(_group,name):(Table,String)|{
            let value=match (captured.as_str(),name.as_str()) {
                ("AutomaticSize","X")=>1,("AutomaticSize","Y")=>2,("AutomaticSize","XY")=>3,
                ("ZIndexBehavior","Sibling")=>1,
                ("ScaleType","Slice")=>1,("ScaleType","Tile")=>2,("ScaleType","Fit")=>3,("ScaleType","Crop")=>4,
                ("TextXAlignment","Center")=>1,("TextXAlignment","Right")=>2,
                ("TextYAlignment","Center")=>1,("TextYAlignment","Bottom")=>2,
                ("EasingDirection","Out")=>1,("EasingDirection","InOut")=>2,
                ("FillDirection","Vertical")=>1,("ScrollingDirection","X")=>1,("ScrollingDirection","Y")=>2,
                ("VerticalScrollBarPosition","Left")=>1,("ResamplerMode","Pixelated")=>1,
                ("ScreenInsets","DeviceSafeInsets")=>1,("ScreenInsets","CoreUISafeInsets")=>2,("ScreenInsets","TopbarSafeInsets")=>3,
                _=>0,
            };
            let item=lua.create_table();item.set("Name",name)?;item.set("Value",value)?;item.set("EnumType",captured.clone())?;Ok(item)
        })?)?;
        group.set_metatable(Some(item_mt));group.raw_set("Name",enum_type)?;Ok(group)
    })?)?;
    let enum_root=lua.create_table();enum_root.set_metatable(Some(root_mt));lua.globals().set("Enum",enum_root)
}

fn make_signal(lua: &Lua) -> LuaResult<Table> {
    let signal=lua.create_table();
    let callbacks: Rc<RefCell<Vec<(Function,bool,Rc<Cell<bool>>)>>>=Rc::new(RefCell::new(Vec::new()));
    let connected=callbacks.clone();
    signal.set("Connect",lua.create_function(move |lua,(_signal,callback):(Table,Function)|{
        let active=Rc::new(Cell::new(true)); connected.borrow_mut().push((callback,false,active.clone()));
        let connection=lua.create_table(); connection.set("Connected",true)?;
        connection.set("Disconnect",lua.create_function(move |_,connection:Table|{active.set(false);connection.set("Connected",false)} )?)?;
        Ok(connection)
    })?)?;
    let once_callbacks=callbacks.clone();
    signal.set("Once",lua.create_function(move |lua,(_signal,callback):(Table,Function)|{
        let active=Rc::new(Cell::new(true)); once_callbacks.borrow_mut().push((callback,true,active.clone()));
        let connection=lua.create_table(); connection.set("Connected",true)?;
        connection.set("Disconnect",lua.create_function(move |_,connection:Table|{active.set(false);connection.set("Connected",false)} )?)?;
        Ok(connection)
    })?)?;
    let fired=callbacks;
    signal.set("Fire",lua.create_function(move |lua,(_signal,args):(Table,Variadic<Value>)|{
        let callbacks=fired.borrow().clone();
        for (callback,once,active) in callbacks{if active.get(){
            if lua.globals().get::<Function>("_arena_step_tasks").is_ok() {let task:Table=lua.globals().get("task")?;let spawn:Function=task.get("spawn")?;let mut values=vec![Value::Function(callback.clone())];values.extend(args.clone());spawn.call::<()>(MultiValue::from_vec(values))?;}
            else{callback.call::<()>(args.clone())?;}
            if once{active.set(false);}
        }}
        fired.borrow_mut().retain(|(_,once,active)|active.get()&&!*once); Ok(())
    })?)?;
    if let Ok(wait)=lua.globals().get::<Function>("_arena_signal_wait") { signal.set("Wait",wait)?; }
    else { signal.set("Wait",lua.create_function(|_,_signal:Table|Ok(Variadic::<Value>::new()))?)?; }
    Ok(signal)
}

fn fire_instance_signal(table:&Table,name:&str,args:Vec<Value>)->LuaResult<()> {
    if let Ok(signal)=table.raw_get::<Table>(name){let fire:Function=signal.get("Fire")?;let mut values=vec![Value::Table(signal)];values.extend(args);fire.call::<()>(MultiValue::from_vec(values))?;}Ok(())
}

fn is_instance_signal(key: &str) -> bool {
    matches!(key,"Activated"|"MouseButton1Click"|"MouseButton1Down"|"MouseButton1Up"|
        "MouseEnter"|"MouseLeave"|"InputBegan"|"InputChanged"|"InputEnded"|
        "Focused"|"FocusLost"|"SelectionGained"|"SelectionLost"|"Changed"|"AncestryChanged"|
        "ChildAdded"|"ChildRemoved"|"DescendantAdded"|"DescendantRemoving"|"Destroying")
}

fn instance_children(this:&Table)->LuaResult<Vec<Table>> {
    let mut children=Vec::new();for pair in this.clone().pairs::<Value,Value>(){let(_,value)=pair?;if let Value::Table(child)=value{if child.raw_get::<Table>("Parent").ok().as_ref()==Some(this){children.push(child);}}}Ok(children)
}

fn class_is_a(class:&str,target:&str)->bool {
    if class==target||target=="Instance" {return true;}
    match target {
        "GuiBase"=>matches!(class,"ScreenGui"|"BillboardGui"|"SurfaceGui"|"GuiObject"|"Frame"|"CanvasGroup"|"ScrollingFrame"|"TextLabel"|"TextButton"|"TextBox"|"ImageLabel"|"ImageButton"|"ViewportFrame"),
        "GuiBase2d"=>matches!(class,"GuiObject"|"Frame"|"CanvasGroup"|"ScrollingFrame"|"TextLabel"|"TextButton"|"TextBox"|"ImageLabel"|"ImageButton"|"ViewportFrame"),
        "LayerCollector"=>matches!(class,"ScreenGui"|"BillboardGui"|"SurfaceGui"),
        "GuiObject"=>matches!(class,"Frame"|"CanvasGroup"|"ScrollingFrame"|"TextLabel"|"TextButton"|"TextBox"|"ImageLabel"|"ImageButton"|"ViewportFrame"),
        "GuiButton"=>matches!(class,"TextButton"|"ImageButton"),
        "GuiLabel"=>matches!(class,"TextLabel"|"ImageLabel"),
        "UIComponent"=>class.starts_with("UI"),
        "LuaSourceContainer"=>matches!(class,"LocalScript"|"ModuleScript"|"Script"),
        "BaseScript"=>matches!(class,"LocalScript"|"Script"),
        _=>false,
    }
}

fn make_instance(lua: &Lua, class: &str, name: &str) -> LuaResult<Table> {
    let t = lua.create_table();
    t.set("Name", name)?;
    t.set("ClassName", class)?;
    let noop = lua.create_function(|_, _: Variadic<Value>| Ok(Variadic::<Value>::new()))?;
    for m in ["GetActor","Clone","Destroy"] {t.set(m,noop.clone())?;}
    t.set("FindFirstChild",lua.create_function(|_,(this,name,recursive):(Table,String,Option<bool>)|{
        if let Ok(value)=this.raw_get::<Value>(&name){if !matches!(value,Value::Nil){return Ok(value);}}
        if recursive.unwrap_or(false){let mut stack=instance_children(&this)?;while let Some(child)=stack.pop(){if child.raw_get::<String>("Name").ok().as_deref()==Some(name.as_str()){return Ok(Value::Table(child));}stack.extend(instance_children(&child)?);}}
        Ok(Value::Nil)
    })?)?;
    t.set("WaitForChild",lua.create_function(|_,(this,name,_timeout):(Table,String,Option<f64>)|this.raw_get::<Value>(&name).or(Ok(Value::Nil)))?)?;
    t.set("GetChildren",lua.create_function(|_,this:Table|instance_children(&this))?)?;
    t.set("GetDescendants",lua.create_function(|_,this:Table|{let mut descendants=Vec::new();let mut stack=instance_children(&this)?;while let Some(child)=stack.pop(){stack.extend(instance_children(&child)?);descendants.push(child);}Ok(descendants)})?)?;
    t.set("FindFirstChildWhichIsA",lua.create_function(|_,(this,class,recursive):(Table,String,Option<bool>)|{let mut stack=instance_children(&this)?;while let Some(child)=stack.pop(){if class_is_a(&child.raw_get::<String>("ClassName")?,&class){return Ok(Some(child));}if recursive.unwrap_or(false){stack.extend(instance_children(&child)?);}}Ok(None)})?)?;
    t.set("FindFirstChildOfClass",lua.create_function(|_,(this,class):(Table,String)|{for child in instance_children(&this)?{if child.raw_get::<String>("ClassName")?==class{return Ok(Some(child));}}Ok(None)})?)?;
    t.set("GetFullName",lua.create_function(|_,this:Table|{let mut names=vec![this.raw_get::<String>("Name")?];let mut current=this.raw_get::<Table>("Parent").ok();while let Some(parent)=current{names.push(parent.raw_get::<String>("Name")?);current=parent.raw_get::<Table>("Parent").ok();}names.reverse();Ok(names.join("."))})?)?;
    t.set("IsDescendantOf",lua.create_function(|_,(this,ancestor):(Table,Table)|{let mut current=this.raw_get::<Table>("Parent").ok();while let Some(parent)=current{if parent==ancestor{return Ok(true);}current=parent.raw_get::<Table>("Parent").ok();}Ok(false)})?)?;
    t.set("IsAncestorOf",lua.create_function(|_,(this,descendant):(Table,Table)|{let mut current=descendant.raw_get::<Table>("Parent").ok();while let Some(parent)=current{if parent==this{return Ok(true);}current=parent.raw_get::<Table>("Parent").ok();}Ok(false)})?)?;
    t.set("FindFirstAncestor",lua.create_function(|_,(this,name):(Table,String)|{let mut current=this.raw_get::<Table>("Parent").ok();while let Some(parent)=current{if parent.raw_get::<String>("Name")?==name{return Ok(Some(parent));}current=parent.raw_get::<Table>("Parent").ok();}Ok(None)})?)?;
    t.set("FindFirstAncestorWhichIsA",lua.create_function(|_,(this,class):(Table,String)|{let mut current=this.raw_get::<Table>("Parent").ok();while let Some(parent)=current{if class_is_a(&parent.raw_get::<String>("ClassName")?,&class){return Ok(Some(parent));}current=parent.raw_get::<Table>("Parent").ok();}Ok(None)})?)?;
    t.set("FindFirstAncestorOfClass",lua.create_function(|_,(this,class):(Table,String)|{let mut current=this.raw_get::<Table>("Parent").ok();while let Some(parent)=current{if parent.raw_get::<String>("ClassName")?==class{return Ok(Some(parent));}current=parent.raw_get::<Table>("Parent").ok();}Ok(None)})?)?;
    let attributes=lua.create_table();t.raw_set("_attributes",attributes)?;
    t.set("GetAttribute",lua.create_function(|_,(this,name):(Table,String)|this.raw_get::<Table>("_attributes")?.raw_get::<Value>(name))?)?;
    t.set("SetAttribute",lua.create_function(|_,(this,name,value):(Table,String,Value)|{this.raw_get::<Table>("_attributes")?.raw_set(&name,value)?;if let Ok(signal)=this.raw_get::<Table>(&format!("_attribute_signal_{name}")){let fire:Function=signal.get("Fire")?;fire.call::<()>((signal,Variadic::<Value>::new()))?;}Ok(())})?)?;
    t.set("GetAttributeChangedSignal",lua.create_function(|lua,(this,name):(Table,String)|{let key=format!("_attribute_signal_{name}");if let Ok(signal)=this.raw_get::<Table>(&key){return Ok(signal);}let signal=make_signal(lua)?;this.raw_set(key,signal.clone())?;Ok(signal)})?)?;
    t.set("GetAttributes",lua.create_function(|_,this:Table|Ok(this.raw_get::<Table>("_attributes")?))?)?;
    for event in ["Activated","MouseButton1Click","MouseButton1Down","MouseButton1Up","MouseEnter","MouseLeave","InputBegan","InputChanged","InputEnded","Focused","FocusLost","SelectionGained","SelectionLost","Changed","AncestryChanged","ChildAdded","ChildRemoved","DescendantAdded","DescendantRemoving","Destroying"] {
        t.set(event,make_signal(lua)?)?;
    }
    t.set("GetPropertyChangedSignal",lua.create_function(|lua,(this,property):(Table,String)|{
        let key=format!("_property_signal_{property}");
        if let Ok(signal)=this.raw_get::<Table>(&key){return Ok(signal);}let signal=make_signal(lua)?;this.raw_set(key,signal.clone())?;Ok(signal)
    })?)?;
    let class_name = class.to_string();
    let isa = lua.create_function(move |_, (_self, name): (Table, String)| Ok(class_is_a(&class_name,&name)))?;
    t.set("IsA", isa)?;
    let mt = typed_metatable(lua, "Instance")?;
    let _ = t.set_metatable(Some(mt));
    if let Ok(install)=lua.globals().get::<Function>("_arena_install_instance_wait"){install.call::<()>(t.clone())?;}
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


/// Persistent, frame-to-frame Luau state used by the GUI preview play session.
/// Instance tables and connected callbacks remain alive until another place is loaded.
const GUI_SYNC_PROPERTIES:&[&str]=&["Visible","Position","Size","AnchorPoint","Rotation","BackgroundColor3","BackgroundTransparency","Text","TextColor3","TextTransparency","Image","ImageColor3","ImageTransparency","CanvasPosition","CanvasSize","Enabled"];

fn clone_runtime_value(lua:&Lua,value:Value)->LuaResult<Value>{
    let Value::Table(source)=value else{return Ok(value);};
    if source.raw_get::<String>("ClassName").is_ok()||source.raw_get::<Function>("Connect").is_ok(){return Ok(Value::Table(source));}
    let copy=lua.create_table();for pair in source.clone().pairs::<Value,Value>(){let(key,value)=pair?;copy.raw_set(key,clone_runtime_value(lua,value)?)?;}if let Some(metatable)=source.get_metatable(){copy.set_metatable(Some(metatable))?;}Ok(Value::Table(copy))
}

fn clone_runtime_instance(lua:&Lua,source:&Table,queue:Rc<RefCell<Vec<Table>>>,parent:Option<Table>)->LuaResult<Table>{
    let class=source.raw_get::<String>("ClassName").unwrap_or_else(|_|"Folder".into());
    let name=source.raw_get::<String>("Name").unwrap_or_else(|_|class.clone());
    let clone=make_instance(lua,&class,&name)?;
    if let Some(parent)=parent {clone.raw_set("Parent",parent.clone())?;parent.raw_set(name.as_str(),clone.clone())?;}
    for pair in source.clone().pairs::<Value,Value>() {
        let (key,value)=pair?;let Value::String(key_string)=&key else{continue;};let key_name=key_string.to_str()?;
        if key_name=="_ref"||key_name=="_destroyed"||key_name.starts_with("_property_signal_")||matches!(key_name.as_str(),"Name"|"ClassName"|"Parent"|"Clone"|"Destroy")||matches!(value,Value::Function(_)){continue;}
        if let Value::Table(table)=&value {if table.raw_get::<Function>("Connect").is_ok(){continue;}if table.raw_get::<Table>("Parent").ok().as_ref()==Some(source){continue;}}
        clone.raw_set(key,clone_runtime_value(lua,value)?)?;
    }
    queue.borrow_mut().push(clone.clone());
    for pair in source.clone().pairs::<Value,Value>() {let(_,value)=pair?;if let Value::Table(child)=value{if child.raw_get::<Table>("Parent").ok().as_ref()==Some(source){clone_runtime_instance(lua,&child,queue.clone(),Some(clone.clone()))?;}}}
    Ok(clone)
}

struct ActiveGuiTween {
    target: Table,
    goals: Vec<(String,Value,Value)>,
    elapsed: f32,
    duration: f32,
    repeat_count: i32,
    reverses: bool,
    easing_style: String,
    easing_direction: String,
    control: Rc<Cell<u8>>, // 0 running, 1 paused, 2 cancelled
    completed: Table,
}

pub struct GuiPlaySession {
    lua: Lua,
    instances: std::collections::HashMap<DomRef, Table>,
    synchronized_properties: std::collections::HashMap<DomRef, Vec<String>>,
    active_tweens: Rc<RefCell<Vec<ActiveGuiTween>>>,
    pending_instances: Rc<RefCell<Vec<Table>>>,
    pending_destructions: Rc<RefCell<Vec<Table>>>,
    scheduler_step: Function,
    run_service: Table,
    user_input_service: Table,
    respawn_requested: Rc<Cell<bool>>,
    respawn_properties: std::collections::HashMap<DomRef,Vec<(rbx_dom_weak::Ustr,DomVariant)>>,
    last_tick: std::time::Instant,
}

fn preserve_variant_type(existing:Option<&DomVariant>,value:DomVariant)->DomVariant {
    match (existing,value) {
        (Some(DomVariant::Float32(_)),DomVariant::Float64(value))=>DomVariant::Float32(value as f32),
        (Some(DomVariant::Int32(_)),DomVariant::Float64(value))=>DomVariant::Int32(value.round() as i32),
        (Some(DomVariant::Int64(_)),DomVariant::Float64(value))=>DomVariant::Int64(value.round() as i64),
        (Some(DomVariant::Int32(_)),DomVariant::Int64(value))=>DomVariant::Int32(value as i32),
        (Some(DomVariant::Float32(_)),DomVariant::Int64(value))=>DomVariant::Float32(value as f32),
        (Some(DomVariant::Float64(_)),DomVariant::Int64(value))=>DomVariant::Float64(value as f64),
        (Some(DomVariant::Enum(_)),DomVariant::Float64(value))=>DomVariant::Enum(rbx_dom_weak::types::Enum::from_u32(value.max(0.0) as u32)),
        (Some(DomVariant::Enum(_)),DomVariant::Int64(value))=>DomVariant::Enum(rbx_dom_weak::types::Enum::from_u32(value.max(0) as u32)),
        (_,value)=>value,
    }
}

fn ease_gui_tween(amount:f32,style:&str,direction:&str)->f32 {
    let curve=|value:f32|match style {"Quad"=>value*value,"Cubic"=>value*value*value,"Quart"=>value.powi(4),"Quint"=>value.powi(5),"Sine"=>1.0-(value*std::f32::consts::FRAC_PI_2).cos(),"Exponential"=>if value<=0.0{0.0}else{2.0f32.powf(10.0*(value-1.0))},"Circular"=>1.0-(1.0-value*value).max(0.0).sqrt(),_=>value};
    match direction {"In"=>curve(amount),"InOut"=>if amount<0.5{curve(amount*2.0)*0.5}else{1.0-curve((1.0-amount)*2.0)*0.5},_=>1.0-curve(1.0-amount)}
}

fn interpolate_gui_value(lua:&Lua,start:&Value,end:&Value,amount:f32)->LuaResult<Value>{
    Ok(match (start,end) {
        (Value::Number(a),Value::Number(b))=>Value::Number(a+(b-a)*amount as f64),
        (Value::Integer(a),Value::Integer(b))=>Value::Integer((*a as f64+(*b as f64-*a as f64)*amount as f64).round() as i64),
        (Value::Table(a),Value::Table(b))=>{let result=lua.create_table();for pair in b.clone().pairs::<Value,Value>(){let(key,end)=pair?;let start=a.raw_get::<Value>(key.clone()).unwrap_or(Value::Nil);result.raw_set(key,interpolate_gui_value(lua,&start,&end,amount)?)?;}Value::Table(result)},
        _=>if amount>=1.0{end.clone()}else{start.clone()},
    })
}

impl GuiPlaySession {
    pub fn new(dom: &WeakDom) -> Result<Self, String> {
        let lua=build_vm().map_err(|error|error.to_string())?;
        let active_tweens=Rc::new(RefCell::new(Vec::<ActiveGuiTween>::new()));
        let pending_instances=Rc::new(RefCell::new(Vec::<Table>::new()));
        let pending_destructions=Rc::new(RefCell::new(Vec::<Table>::new()));
        let respawn_requested=Rc::new(Cell::new(false));
        let mut instances=std::collections::HashMap::new();
        fn create(lua:&Lua,dom:&WeakDom,referent:DomRef,instances:&mut std::collections::HashMap<DomRef,Table>,creations:Rc<RefCell<Vec<Table>>>,destructions:Rc<RefCell<Vec<Table>>>)->LuaResult<()> {
            let Some(instance)=dom.get_by_ref(referent) else{return Ok(());};
            let table=make_instance(lua,&instance.class,&instance.name)?;
            table.raw_set("_ref",ref_to_i64(referent))?;
            let destroy_table=table.clone();let destroy_queue=destructions.clone();
            table.raw_set("Destroy",lua.create_function(move |_,_this:Table|{if let Ok(signal)=destroy_table.raw_get::<Table>("Destroying"){let fire:Function=signal.get("Fire")?;fire.call::<()>((signal,Variadic::<Value>::new()))?;}destroy_queue.borrow_mut().push(destroy_table.clone());Ok(())})?)?;
            let clone_table=table.clone();let clone_queue=creations.clone();
            table.raw_set("Clone",lua.create_function(move |lua,_this:Table|clone_runtime_instance(lua,&clone_table,clone_queue.clone(),None))?)?;
            for (key,value) in &instance.properties { if let Ok(value)=variant_to_value(lua,value){table.raw_set(key.as_str(),value)?;} }
            instances.insert(referent,table);
            for child in instance.children(){create(lua,dom,*child,instances,creations.clone(),destructions.clone())?;} Ok(())
        }
        create(&lua,dom,dom.root_ref(),&mut instances,pending_instances.clone(),pending_destructions.clone()).map_err(|error|error.to_string())?;
        for (referent,table) in &instances {
            let Some(instance)=dom.get_by_ref(*referent) else{continue;};
            if let Some(parent)=instances.get(&instance.parent()){table.raw_set("Parent",parent.clone()).map_err(|error|error.to_string())?;}
            for child in instance.children(){if let (Some(child_instance),Some(child_table))=(dom.get_by_ref(*child),instances.get(child)){table.raw_set(child_instance.name.as_str(),child_table.clone()).map_err(|error|error.to_string())?;}}
        }
        let run_service=make_instance(&lua,"RunService","RunService").map_err(|error|error.to_string())?;
        for event in ["Heartbeat","RenderStepped","Stepped"] {run_service.raw_set(event,make_signal(&lua).map_err(|error|error.to_string())?).map_err(|error|error.to_string())?;}
        let user_input_service=make_instance(&lua,"UserInputService","UserInputService").map_err(|error|error.to_string())?;
        for event in ["InputBegan","InputChanged","InputEnded","TouchStarted","TouchMoved","TouchEnded","TextBoxFocused","TextBoxFocusReleased"] {user_input_service.raw_set(event,make_signal(&lua).map_err(|error|error.to_string())?).map_err(|error|error.to_string())?;}
        user_input_service.raw_set("TouchEnabled",cfg!(target_os="android")).map_err(|error|error.to_string())?;
        user_input_service.raw_set("KeyboardEnabled",true).map_err(|error|error.to_string())?;
        user_input_service.raw_set("MouseEnabled",true).map_err(|error|error.to_string())?;
        if let Some(game)=instances.get(&dom.root_ref()) {
            lua.globals().set("game",game.clone()).map_err(|error|error.to_string())?;
            game.raw_set("RunService",run_service.clone()).map_err(|error|error.to_string())?;
            lua.globals().set("RunService",run_service.clone()).map_err(|error|error.to_string())?;
            game.raw_set("UserInputService",user_input_service.clone()).map_err(|error|error.to_string())?;
            lua.globals().set("UserInputService",user_input_service.clone()).map_err(|error|error.to_string())?;
            let tween_service=make_instance(&lua,"TweenService","TweenService").map_err(|error|error.to_string())?;
            let tween_queue=active_tweens.clone();
            tween_service.set("Create",lua.create_function(move |lua,(_service,target,info,goals):(Table,Table,Table,Table)|{
                let tween=lua.create_table(); let completed=make_signal(lua)?; tween.set("Completed",completed.clone())?;
                let duration=info.get::<f64>("Time").unwrap_or(1.0).max(0.0) as f32;
                let delay=info.get::<f64>("DelayTime").unwrap_or(0.0).max(0.0) as f32;
                let repeat_count=info.get::<i64>("RepeatCount").unwrap_or(0) as i32;
                let reverses=info.get::<bool>("Reverses").unwrap_or(false);
                let enum_name=|key:&str,fallback:&str|info.get::<Table>(key).ok().and_then(|value|value.get::<String>("Name").ok()).unwrap_or_else(||fallback.to_string());
                let easing_style=enum_name("EasingStyle","Linear");let easing_direction=enum_name("EasingDirection","Out");
                let control=Rc::new(Cell::new(0u8));let started=Rc::new(Cell::new(false));
                let queue=tween_queue.clone();let play_control=control.clone();let play_started=started.clone();
                tween.set("Play",lua.create_function(move |_,_tween:Table|{
                    if play_started.replace(true){play_control.set(0);return Ok(());}
                    play_control.set(0);let mut values=Vec::new();
                    for pair in goals.clone().pairs::<Value,Value>() { let (key,end)=pair?; if let Value::String(key)=key { let key=key.to_str()?;let start=target.raw_get::<Value>(key.as_str()).unwrap_or(Value::Nil);values.push((key,start,end)); } }
                    queue.borrow_mut().push(ActiveGuiTween{target:target.clone(),goals:values,elapsed:-delay,duration,repeat_count,reverses,easing_style:easing_style.clone(),easing_direction:easing_direction.clone(),control:play_control.clone(),completed:completed.clone()});Ok(())
                })?)?;
                let pause_control=control.clone();tween.set("Pause",lua.create_function(move |_,_tween:Table|{pause_control.set(1);Ok(())})?)?;
                let cancel_control=control;tween.set("Cancel",lua.create_function(move |_,_tween:Table|{cancel_control.set(2);Ok(())})?)?; Ok(tween)
            }).map_err(|error|error.to_string())?).map_err(|error|error.to_string())?;
            game.raw_set("TweenService",tween_service.clone()).map_err(|error|error.to_string())?;
            lua.globals().set("TweenService",tween_service).map_err(|error|error.to_string())?;
            let game_table=game.clone();
            game.set("GetService",lua.create_function(move |lua,(_game,name):(Table,String)|{
                game_table.raw_get::<Value>(&name).or_else(|_|Ok(Value::Table(make_instance(lua,&name,&name)?)))
            }).map_err(|error|error.to_string())?).map_err(|error|error.to_string())?;
        }
        // Build the client-side Players.LocalPlayer.PlayerGui view and clone
        // StarterGui's ScreenGuis into it. The retained tables keep their DOM
        // referents so viewport events still dispatch to the correct callbacks.
        let players=dom.root().children().iter().find_map(|referent|dom.get_by_ref(*referent)
            .filter(|instance|instance.class=="Players").and_then(|_|instances.get(referent).cloned()))
            .unwrap_or(make_instance(&lua,"Players","Players").map_err(|error|error.to_string())?);
        let local_player=make_instance(&lua,"Player","LocalPlayer").map_err(|error|error.to_string())?;
        let respawn_flag=respawn_requested.clone();
        local_player.raw_set("LoadCharacter",lua.create_function(move |_,_player:Table|{respawn_flag.set(true);Ok(())}).map_err(|error|error.to_string())?).map_err(|error|error.to_string())?;
        let player_gui=instances.iter().find_map(|(referent,table)|dom.get_by_ref(*referent)
            .filter(|instance|instance.class=="PlayerGui").map(|_|table.clone()))
            .unwrap_or(make_instance(&lua,"PlayerGui","PlayerGui").map_err(|error|error.to_string())?);
        players.raw_set("LocalPlayer",local_player.clone()).map_err(|error|error.to_string())?;
        local_player.raw_set("Parent",players.clone()).map_err(|error|error.to_string())?;
        local_player.raw_set("PlayerGui",player_gui.clone()).map_err(|error|error.to_string())?;
        player_gui.raw_set("Parent",local_player.clone()).map_err(|error|error.to_string())?;
        if let Some(starter_ref)=dom.root().children().iter().find(|referent|dom.get_by_ref(**referent).is_some_and(|instance|instance.class=="StarterGui")) {
            if let Some(starter)=dom.get_by_ref(*starter_ref) {
                for child in starter.children() {
                    if let (Some(instance),Some(table))=(dom.get_by_ref(*child),instances.get(child)) {
                        player_gui.raw_set(instance.name.as_str(),table.clone()).map_err(|error|error.to_string())?;
                        table.raw_set("Parent",player_gui.clone()).map_err(|error|error.to_string())?;
                    }
                }
            }
        }
        if let Some(game)=instances.get(&dom.root_ref()) { game.raw_set("Players",players.clone()).map_err(|error|error.to_string())?; }
        let runtime_instance=lua.create_table().map_err(|error|error.to_string())?;
        let creation_queue=pending_instances.clone();let destruction_queue=pending_destructions.clone();
        runtime_instance.set("new",lua.create_function(move |lua,(class,parent):(String,Option<Table>)|{
            let table=make_instance(lua,&class,&class)?;
            let destroy_table=table.clone();let destroy_queue=destruction_queue.clone();
            table.raw_set("Destroy",lua.create_function(move |_,_this:Table|{if let Ok(signal)=destroy_table.raw_get::<Table>("Destroying"){let fire:Function=signal.get("Fire")?;fire.call::<()>((signal,Variadic::<Value>::new()))?;}destroy_queue.borrow_mut().push(destroy_table.clone());Ok(())})?)?;
            let clone_table=table.clone();let clone_queue=creation_queue.clone();
            table.raw_set("Clone",lua.create_function(move |lua,_this:Table|clone_runtime_instance(lua,&clone_table,clone_queue.clone(),None))?)?;
            if let Some(parent)=parent { table.raw_set("Parent",parent.clone())?; parent.raw_set(class.as_str(),table.clone())?; }
            creation_queue.borrow_mut().push(table.clone()); Ok(table)
        }).map_err(|error|error.to_string())?).map_err(|error|error.to_string())?;
        lua.globals().set("Instance",runtime_instance).map_err(|error|error.to_string())?;
        lua.load(r#"
            local waiting = {}
            local function schedule(thread, delay, args, started)
                table.insert(waiting, {thread=thread, remaining=math.max(tonumber(delay) or 0, 0), elapsed=0, args=args, started=started or false})
                return thread
            end
            local function resumeTask(record)
                if record.cancelled then return end
                local ok, delay
                if record.started then ok, delay = coroutine.resume(record.thread, record.elapsed)
                else record.started=true; ok, delay = coroutine.resume(record.thread, table.unpack(record.args, 1, record.args.n)) end
                if not ok then warn(delay); return end
                if coroutine.status(record.thread) ~= "dead" then schedule(record.thread, delay, table.pack(), true) end
            end
            task = {}
            function task.spawn(callback, ...)
                local record={thread=coroutine.create(callback),remaining=0,elapsed=0,args=table.pack(...),started=false}
                resumeTask(record); return record.thread
            end
            function task.defer(callback, ...) return schedule(coroutine.create(callback), 0, table.pack(...)) end
            function task.delay(duration, callback, ...) return schedule(coroutine.create(callback), duration, table.pack(...)) end
            function task.wait(duration) return coroutine.yield(math.max(tonumber(duration) or 0, 0)) end
            function task.cancel(thread) for _,record in ipairs(waiting) do if record.thread==thread then record.cancelled=true end end end
            function _arena_step_tasks(delta)
                local ready = {}
                local remove = {}
                for index, record in ipairs(waiting) do
                    record.remaining -= delta
                    record.elapsed += delta
                    if record.cancelled then table.insert(remove, index)
                    elseif record.remaining <= 0 then table.insert(remove, index); table.insert(ready, record) end
                end
                for index=#remove,1,-1 do table.remove(waiting, remove[index]) end
                -- Resume in insertion order. New waits created by these
                -- callbacks remain queued until the next scheduler step.
                for _, record in ipairs(ready) do resumeTask(record) end
            end
            function _arena_resume_task(thread, ...)
                task.cancel(thread)
                local ok, delay = coroutine.resume(thread, ...)
                if not ok then warn(delay); return end
                if coroutine.status(thread) ~= "dead" then schedule(thread, delay, table.pack(), true) end
            end
            function _arena_signal_wait(signal)
                local thread = coroutine.running()
                local connection
                connection = signal:Connect(function(...)
                    connection:Disconnect()
                    _arena_resume_task(thread, ...)
                end)
                return coroutine.yield(math.huge)
            end
            function _arena_install_instance_wait(instance)
                instance.WaitForChild = function(self, name, timeout)
                    local child = rawget(self, name)
                    if child ~= nil then return child end
                    local elapsed = 0
                    while timeout == nil or elapsed < timeout do
                        elapsed += task.wait()
                        child = rawget(self, name)
                        if child ~= nil then return child end
                    end
                    return nil
                end
            end
        "#).exec().map_err(|error|error.to_string())?;
        let install_wait:Function=lua.globals().get("_arena_install_instance_wait").map_err(|error|error.to_string())?;
        for table in instances.values(){install_wait.call::<()>(table.clone()).map_err(|error|error.to_string())?;}
        let scheduler_step:Function=lua.globals().get("_arena_step_tasks").map_err(|error|error.to_string())?;
        let signal_wait:Function=lua.globals().get("_arena_signal_wait").map_err(|error|error.to_string())?;
        let upgrade_signals=|table:&Table|->Result<(),String>{for pair in table.clone().pairs::<Value,Value>(){let(_,value)=pair.map_err(|error|error.to_string())?;if let Value::Table(candidate)=value{if candidate.raw_get::<Function>("Connect").is_ok()&&candidate.raw_get::<Function>("Fire").is_ok(){candidate.raw_set("Wait",signal_wait.clone()).map_err(|error|error.to_string())?;}}}Ok(())};
        for table in instances.values(){upgrade_signals(table)?;}upgrade_signals(&run_service)?;upgrade_signals(&user_input_service)?;upgrade_signals(&players)?;upgrade_signals(&local_player)?;upgrade_signals(&player_gui)?;

        // Roblox ModuleScript require with one-time result caching. Module
        // environments are isolated like LocalScripts while sharing _G.
        let module_sources:std::collections::HashMap<DomRef,String>=instances.keys().filter_map(|referent|dom.get_by_ref(*referent).filter(|instance|instance.class=="ModuleScript").and_then(|instance|match instance.properties.get(&rbx_dom_weak::ustr("Source")){Some(DomVariant::String(source))=>Some((*referent,source.clone())),_=>None})).collect();
        let require_instances=instances.clone();
        let module_cache:Rc<RefCell<std::collections::HashMap<DomRef,Value>>>=Rc::new(RefCell::new(std::collections::HashMap::new()));
        let require_cache=module_cache.clone();
        let loading_modules:Rc<RefCell<std::collections::HashSet<DomRef>>>=Rc::new(RefCell::new(std::collections::HashSet::new()));
        let require_loading=loading_modules;
        lua.globals().set("require",lua.create_function(move |lua,module:Value|{
            let Value::Table(module)=module else{return Err(LuaError::runtime("require expects a ModuleScript"));};
            let Some(referent)=table_to_ref(&module)? else{return Err(LuaError::runtime("require expects a retained ModuleScript"));};
            if let Some(value)=require_cache.borrow().get(&referent){return Ok(value.clone());}
            if !require_loading.borrow_mut().insert(referent){return Err(LuaError::runtime("ModuleScript requested recursively"));}
            let source=module_sources.get(&referent).cloned().or_else(||module.raw_get::<String>("Source").ok());
            let Some(source)=source else{require_loading.borrow_mut().remove(&referent);return Err(LuaError::runtime("required Instance is not a ModuleScript"));};
            let environment=lua.create_table();environment.raw_set("script",require_instances.get(&referent).cloned().unwrap_or(module))?;environment.raw_set("_G",lua.globals())?;
            let metatable=lua.create_table();metatable.raw_set("__index",lua.globals())?;environment.set_metatable(Some(metatable))?;
            let result=lua.load(&source).set_name("ModuleScript").set_environment(environment).eval::<Value>();
            require_loading.borrow_mut().remove(&referent);
            let value=result?;if matches!(value,Value::Nil){return Err(LuaError::runtime("ModuleScript did not return exactly one value"));}require_cache.borrow_mut().insert(referent,value.clone());Ok(value)
        }).map_err(|error|error.to_string())?).map_err(|error|error.to_string())?;

        // Execute LocalScripts once. Their signal connections remain retained by
        // these Instance tables and are fired from viewport events every frame.
        for (referent,table) in &instances {
            let Some(instance)=dom.get_by_ref(*referent) else{continue;};
            if instance.class!="LocalScript"{continue;}
            let Some(DomVariant::String(source))=instance.properties.get(&rbx_dom_weak::ustr("Source")) else{continue;};
            let environment=lua.create_table().map_err(|error|error.to_string())?;
            environment.raw_set("script",table.clone()).map_err(|error|error.to_string())?;
            environment.raw_set("_G",lua.globals()).map_err(|error|error.to_string())?;
            let metatable=lua.create_table().map_err(|error|error.to_string())?;
            metatable.raw_set("__index",lua.globals()).map_err(|error|error.to_string())?;
            environment.set_metatable(Some(metatable)).map_err(|error|error.to_string())?;
            match lua.load(source).set_name(instance.name.as_str()).set_environment(environment).into_function() {
                Ok(function)=>{let task:Table=lua.globals().get("task").map_err(|error|error.to_string())?;let spawn:Function=task.get("spawn").map_err(|error|error.to_string())?;if let Err(error)=spawn.call::<()>(function){with_log(|log|log.push(OutputLine{level:Level::Error,text:format!("{}: {error}",instance.name)}));}},
                Err(error)=>with_log(|log|log.push(OutputLine{level:Level::Error,text:format!("{}: {error}",instance.name)})),
            }
        }
        let synchronized_properties=instances.keys().map(|referent|{
            let mut names:Vec<String>=dom.get_by_ref(*referent).map(|instance|instance.properties.keys().map(|key|key.as_str().to_string()).collect()).unwrap_or_default();
            for name in GUI_SYNC_PROPERTIES {if !names.iter().any(|existing|existing==name){names.push((*name).to_string());}}
            (*referent,names)
        }).collect();
        let mut respawn_properties=std::collections::HashMap::new();
        fn snapshot_tree(dom:&WeakDom,referent:DomRef,out:&mut std::collections::HashMap<DomRef,Vec<(rbx_dom_weak::Ustr,DomVariant)>>){if let Some(instance)=dom.get_by_ref(referent){out.insert(referent,instance.properties.iter().map(|(key,value)|(key.clone(),value.clone())).collect());for child in instance.children(){snapshot_tree(dom,*child,out);}}}
        if let Some(starter)=dom.root().children().iter().find_map(|referent|dom.get_by_ref(*referent).filter(|instance|instance.class=="StarterGui")) {for child in starter.children(){if dom.get_by_ref(*child).is_some_and(|instance|instance.class=="ScreenGui"&&instance.properties.get(&rbx_dom_weak::ustr("ResetOnSpawn")).map(|value|!matches!(value,DomVariant::Bool(false))).unwrap_or(true)){snapshot_tree(dom,*child,&mut respawn_properties);}}}
        Ok(Self{lua,instances,synchronized_properties,active_tweens,pending_instances,pending_destructions,scheduler_step,run_service,user_input_service,respawn_requested,respawn_properties,last_tick:std::time::Instant::now()})
    }

    pub fn take_respawn_request(&self)->bool{self.respawn_requested.replace(false)}

    pub fn restore_for_respawn(&self,dom:&mut WeakDom){
        let roots:Vec<DomRef>=self.respawn_properties.keys().copied().filter(|referent|dom.get_by_ref(*referent).is_some_and(|instance|instance.class=="ScreenGui")).collect();
        let mut remove=Vec::new();for root in roots{let mut stack=dom.get_by_ref(root).map(|instance|instance.children().to_vec()).unwrap_or_default();while let Some(child)=stack.pop(){if self.respawn_properties.contains_key(&child){if let Some(instance)=dom.get_by_ref(child){stack.extend_from_slice(instance.children());}}else{remove.push(child);}}}for referent in remove{if dom.get_by_ref(referent).is_some(){dom.destroy(referent);}}
        for (referent,properties) in &self.respawn_properties{if let Some(instance)=dom.get_by_ref_mut(*referent){instance.properties.clear();for (key,value) in properties{instance.properties.insert(key.clone(),value.clone());}}}
    }

    pub fn tick(&mut self)->Result<(),String>{
        let now=std::time::Instant::now();let delta=(now-self.last_tick).as_secs_f32().min(0.1);self.last_tick=now;
        for event in ["RenderStepped","Stepped","Heartbeat"] {if let Ok(signal)=self.run_service.raw_get::<Table>(event){let fire:Function=signal.get("Fire").map_err(|error|error.to_string())?;fire.call::<()>((signal,delta as f64)).map_err(|error|error.to_string())?;}}
        self.scheduler_step.call::<()>(delta as f64).map_err(|error|error.to_string())?;
        let mut completed=Vec::new();
        {
            let mut tweens=self.active_tweens.borrow_mut();
            for tween in tweens.iter_mut(){
                if tween.control.get()!=0{continue;}tween.elapsed+=delta;if tween.elapsed<0.0{continue;}
                let iteration_duration=tween.duration.max(0.0001)*if tween.reverses{2.0}else{1.0};
                let total_duration=if tween.repeat_count<0{f32::INFINITY}else{iteration_duration*(tween.repeat_count+1)as f32};
                let finished=tween.elapsed>=total_duration;
                let within=tween.elapsed%iteration_duration;
                let mut linear=if tween.duration<=0.0{1.0}else{(within/tween.duration).clamp(0.0,1.0)};
                if tween.reverses&&within>=tween.duration{linear=1.0-((within-tween.duration)/tween.duration.max(0.0001)).clamp(0.0,1.0);}
                if finished{linear=if tween.reverses{0.0}else{1.0};}
                let amount=ease_gui_tween(linear,&tween.easing_style,&tween.easing_direction);
                for(key,start,end)in &tween.goals{let value=interpolate_gui_value(&self.lua,start,end,amount).map_err(|error|error.to_string())?;tween.target.raw_set(key.as_str(),value).map_err(|error|error.to_string())?;}
                if finished{completed.push(tween.completed.clone());tween.control.set(2);}
            }
            tweens.retain(|tween|tween.control.get()!=2);
        }
        for signal in completed{let fire:Function=signal.get("Fire").map_err(|error|error.to_string())?;fire.call::<()>((signal,Variadic::<Value>::new())).map_err(|error|error.to_string())?;}
        Ok(())
    }

    pub fn fire_keyboard_input(&self,key_name:&str,began:bool,processed:bool)->Result<(),String>{
        let input=self.lua.create_table().map_err(|error|error.to_string())?;
        let input_type=self.lua.create_table().map_err(|error|error.to_string())?;input_type.set("Name","Keyboard").map_err(|error|error.to_string())?;
        let key_code=self.lua.create_table().map_err(|error|error.to_string())?;key_code.set("Name",key_name).map_err(|error|error.to_string())?;
        let state=self.lua.create_table().map_err(|error|error.to_string())?;state.set("Name",if began{"Begin"}else{"End"}).map_err(|error|error.to_string())?;
        input.set("UserInputType",input_type).map_err(|error|error.to_string())?;input.set("KeyCode",key_code).map_err(|error|error.to_string())?;input.set("UserInputState",state).map_err(|error|error.to_string())?;
        let event=if began{"InputBegan"}else{"InputEnded"};
        if let Ok(signal)=self.user_input_service.raw_get::<Table>(event){let fire:Function=signal.get("Fire").map_err(|error|error.to_string())?;fire.call::<()>((signal,input,processed)).map_err(|error|error.to_string())?;}Ok(())
    }

    pub fn fire_pointer_changed(&self,referent:DomRef,screen_position:[f32;2],delta:[f32;2])->Result<(),String>{
        let input=self.lua.create_table().map_err(|error|error.to_string())?;
        let input_name=if cfg!(target_os="android"){"Touch"}else{"MouseMovement"};
        let input_type=self.lua.create_table().map_err(|error|error.to_string())?;input_type.set("Name",input_name).map_err(|error|error.to_string())?;
        let state=self.lua.create_table().map_err(|error|error.to_string())?;state.set("Name","Change").map_err(|error|error.to_string())?;
        let position=self.lua.create_table().map_err(|error|error.to_string())?;position.set("X",screen_position[0]).map_err(|error|error.to_string())?;position.set("Y",screen_position[1]).map_err(|error|error.to_string())?;position.set("Z",0.0).map_err(|error|error.to_string())?;
        let delta_value=self.lua.create_table().map_err(|error|error.to_string())?;delta_value.set("X",delta[0]).map_err(|error|error.to_string())?;delta_value.set("Y",delta[1]).map_err(|error|error.to_string())?;delta_value.set("Z",0.0).map_err(|error|error.to_string())?;
        input.set("UserInputType",input_type).map_err(|error|error.to_string())?;input.set("UserInputState",state).map_err(|error|error.to_string())?;input.set("Position",position).map_err(|error|error.to_string())?;input.set("Delta",delta_value).map_err(|error|error.to_string())?;
        if let Some(instance)=self.instances.get(&referent){if let Ok(signal)=instance.raw_get::<Table>("InputChanged"){let fire:Function=signal.get("Fire").map_err(|error|error.to_string())?;fire.call::<()>((signal,input.clone())).map_err(|error|error.to_string())?;}}
        if let Ok(signal)=self.user_input_service.raw_get::<Table>("InputChanged"){let fire:Function=signal.get("Fire").map_err(|error|error.to_string())?;fire.call::<()>((signal,input.clone(),false)).map_err(|error|error.to_string())?;}
        if input_name=="Touch" {if let Ok(signal)=self.user_input_service.raw_get::<Table>("TouchMoved"){let fire:Function=signal.get("Fire").map_err(|error|error.to_string())?;fire.call::<()>((signal,input,false)).map_err(|error|error.to_string())?;}}
        Ok(())
    }

    pub fn fire_pointer_input(&self,referent:DomRef,began:bool,screen_position:[f32;2])->Result<(),String>{
        let input=self.lua.create_table().map_err(|error|error.to_string())?;
        let input_name=if cfg!(target_os="android"){"Touch"}else{"MouseButton1"};
        let input_type=self.lua.create_table().map_err(|error|error.to_string())?;input_type.set("Name",input_name).map_err(|error|error.to_string())?;
        let state=self.lua.create_table().map_err(|error|error.to_string())?;state.set("Name",if began{"Begin"}else{"End"}).map_err(|error|error.to_string())?;
        let position=self.lua.create_table().map_err(|error|error.to_string())?;position.set("X",screen_position[0]).map_err(|error|error.to_string())?;position.set("Y",screen_position[1]).map_err(|error|error.to_string())?;position.set("Z",0.0).map_err(|error|error.to_string())?;
        input.set("UserInputType",input_type).map_err(|error|error.to_string())?;input.set("UserInputState",state).map_err(|error|error.to_string())?;input.set("Position",position).map_err(|error|error.to_string())?;
        let event=if began{"InputBegan"}else{"InputEnded"};
        if let Some(instance)=self.instances.get(&referent){if let Ok(signal)=instance.raw_get::<Table>(event){let fire:Function=signal.get("Fire").map_err(|error|error.to_string())?;fire.call::<()>((signal,input.clone())).map_err(|error|error.to_string())?;}}
        if let Ok(signal)=self.user_input_service.raw_get::<Table>(event){let fire:Function=signal.get("Fire").map_err(|error|error.to_string())?;fire.call::<()>((signal,input.clone(),false)).map_err(|error|error.to_string())?;}
        if input_name=="Touch" {let touch_event=if began{"TouchStarted"}else{"TouchEnded"};if let Ok(signal)=self.user_input_service.raw_get::<Table>(touch_event){let fire:Function=signal.get("Fire").map_err(|error|error.to_string())?;fire.call::<()>((signal,input,false)).map_err(|error|error.to_string())?;}}
        Ok(())
    }

    pub fn fire_activated(&self,referent:DomRef,screen_position:[f32;2],keyboard:bool)->Result<(),String>{
        let Some(instance)=self.instances.get(&referent) else{return Ok(());};
        let input=self.lua.create_table().map_err(|error|error.to_string())?;
        let input_type=self.lua.create_table().map_err(|error|error.to_string())?;input_type.set("Name",if keyboard{"Keyboard"}else if cfg!(target_os="android"){"Touch"}else{"MouseButton1"}).map_err(|error|error.to_string())?;
        let key_code=self.lua.create_table().map_err(|error|error.to_string())?;key_code.set("Name",if keyboard{"Return"}else{"Unknown"}).map_err(|error|error.to_string())?;
        let position=self.lua.create_table().map_err(|error|error.to_string())?;position.set("X",screen_position[0]).map_err(|error|error.to_string())?;position.set("Y",screen_position[1]).map_err(|error|error.to_string())?;position.set("Z",0.0).map_err(|error|error.to_string())?;
        input.set("UserInputType",input_type).map_err(|error|error.to_string())?;input.set("KeyCode",key_code).map_err(|error|error.to_string())?;input.set("Position",position).map_err(|error|error.to_string())?;
        if let Ok(signal)=instance.raw_get::<Table>("Activated"){let fire:Function=signal.get("Fire").map_err(|error|error.to_string())?;fire.call::<()>((signal,input,1i64)).map_err(|error|error.to_string())?;}Ok(())
    }

    pub fn fire(&self,referent:DomRef,event:&str)->Result<(),String>{
        let Some(instance)=self.instances.get(&referent) else{return Ok(());};
        if let Ok(signal)=instance.raw_get::<Table>(event){let fire:Function=signal.get("Fire").map_err(|error|error.to_string())?;fire.call::<()>((signal,Variadic::<Value>::new())).map_err(|error|error.to_string())?;}
        let service_event=match event{"Focused"=>Some("TextBoxFocused"),"FocusLost"=>Some("TextBoxFocusReleased"),_=>None};
        if let Some(service_event)=service_event{if let Ok(signal)=self.user_input_service.raw_get::<Table>(service_event){let fire:Function=signal.get("Fire").map_err(|error|error.to_string())?;fire.call::<()>((signal,instance.clone())).map_err(|error|error.to_string())?;}}
        Ok(())
    }

    pub fn set_absolute_layout(&self,referent:DomRef,position:[f32;2],size:[f32;2])->Result<(),String>{
        let Some(instance)=self.instances.get(&referent) else{return Ok(());};
        let changed=|name:&str,value:[f32;2]|instance.raw_get::<Table>(name).ok().map(|old|(old.raw_get::<f64>("X").unwrap_or(f64::NAN)-value[0] as f64).abs()>0.01||(old.raw_get::<f64>("Y").unwrap_or(f64::NAN)-value[1] as f64).abs()>0.01).unwrap_or(true);
        let position_changed=changed("AbsolutePosition",position);let size_changed=changed("AbsoluteSize",size);
        let position_value=variant_to_value(&self.lua,&DomVariant::Vector2(ty::Vector2::new(position[0],position[1]))).map_err(|error|error.to_string())?;
        let size_value=variant_to_value(&self.lua,&DomVariant::Vector2(ty::Vector2::new(size[0],size[1]))).map_err(|error|error.to_string())?;
        instance.raw_set("AbsolutePosition",position_value).map_err(|error|error.to_string())?;
        instance.raw_set("AbsoluteSize",size_value).map_err(|error|error.to_string())?;
        for (name,did_change) in [("AbsolutePosition",position_changed),("AbsoluteSize",size_changed)] {if did_change{fire_instance_signal(instance,"Changed",vec![Value::String(self.lua.create_string(name).map_err(|error|error.to_string())?)]).map_err(|error|error.to_string())?;fire_instance_signal(instance,&format!("_property_signal_{name}"),Vec::new()).map_err(|error|error.to_string())?;}}
        Ok(())
    }

    pub fn set_text(&self,referent:DomRef,text:&str)->Result<(),String>{
        let Some(instance)=self.instances.get(&referent) else{return Ok(());};
        instance.raw_set("Text",text).map_err(|error|error.to_string())?;
        if let Ok(signal)=instance.raw_get::<Table>("Changed") { let fire:Function=signal.get("Fire").map_err(|error|error.to_string())?;fire.call::<()>((signal,"Text")).map_err(|error|error.to_string())?; }
        if let Ok(signal)=instance.raw_get::<Table>("_property_signal_Text") { let fire:Function=signal.get("Fire").map_err(|error|error.to_string())?;fire.call::<()>((signal,)).map_err(|error|error.to_string())?; }
        Ok(())
    }

    pub fn synchronize_to_dom(&mut self,dom:&mut WeakDom)->Result<usize,String>{
        let pending:Vec<Table>=self.pending_instances.borrow_mut().drain(..).collect();
        let mut created=0usize;
        for table in pending {
            let parent_table=table.raw_get::<Table>("Parent").ok();
            let parent_ref=parent_table.as_ref().and_then(|parent|table_to_ref(parent).ok().flatten()).unwrap_or_else(||dom.root_ref());
            let class=table.raw_get::<String>("ClassName").unwrap_or_else(|_|"Frame".into());
            let name=table.raw_get::<String>("Name").unwrap_or_else(|_|class.clone());
            let referent=dom.insert(parent_ref,InstanceBuilder::new(class.clone()).with_name(name.clone()));
            table.raw_set("_ref",ref_to_i64(referent)).map_err(|error|error.to_string())?;
            let destroy_table=table.clone();let destroy_queue=self.pending_destructions.clone();
            table.raw_set("Destroy",self.lua.create_function(move |_,_this:Table|{if let Ok(signal)=destroy_table.raw_get::<Table>("Destroying"){let fire:Function=signal.get("Fire")?;fire.call::<()>((signal,Variadic::<Value>::new()))?;}destroy_queue.borrow_mut().push(destroy_table.clone());Ok(())}).map_err(|error|error.to_string())?).map_err(|error|error.to_string())?;
            let clone_table=table.clone();let clone_queue=self.pending_instances.clone();
            table.raw_set("Clone",self.lua.create_function(move |lua,_this:Table|clone_runtime_instance(lua,&clone_table,clone_queue.clone(),None)).map_err(|error|error.to_string())?).map_err(|error|error.to_string())?;
            if let Some(parent)=self.instances.get(&parent_ref){parent.raw_set(name.as_str(),table.clone()).map_err(|error|error.to_string())?;if name!=class{parent.raw_set(class.as_str(),Value::Nil).map_err(|error|error.to_string())?;}fire_instance_signal(parent,"ChildAdded",vec![Value::Table(table.clone())]).map_err(|error|error.to_string())?;let mut ancestor=Some(parent.clone());while let Some(current)=ancestor{fire_instance_signal(&current,"DescendantAdded",vec![Value::Table(table.clone())]).map_err(|error|error.to_string())?;ancestor=current.raw_get::<Table>("Parent").ok();}}
            self.instances.insert(referent,table.clone());
            self.synchronized_properties.insert(referent,GUI_SYNC_PROPERTIES.iter().map(|name|(*name).to_string()).collect());
            if class=="LocalScript" {
                if let Ok(source)=table.raw_get::<String>("Source") {let environment=self.lua.create_table();environment.raw_set("script",table.clone()).map_err(|error|error.to_string())?;environment.raw_set("_G",self.lua.globals()).map_err(|error|error.to_string())?;let metatable=self.lua.create_table();metatable.raw_set("__index",self.lua.globals()).map_err(|error|error.to_string())?;environment.set_metatable(Some(metatable)).map_err(|error|error.to_string())?;let function=self.lua.load(&source).set_name(name.as_str()).set_environment(environment).into_function().map_err(|error|error.to_string())?;let task:Table=self.lua.globals().get("task").map_err(|error|error.to_string())?;let spawn:Function=task.get("spawn").map_err(|error|error.to_string())?;spawn.call::<()>(function).map_err(|error|error.to_string())?;}
            }
            created+=1;
        }
        let destructions:Vec<Table>=self.pending_destructions.borrow_mut().drain(..).collect();
        let mut destroyed=0usize;
        for table in destructions {
            if let Some(referent)=table_to_ref(&table).map_err(|error|error.to_string())? {
                if referent!=dom.root_ref() {
                    if let Some(instance)=dom.get_by_ref(referent){if let Some(parent)=self.instances.get(&instance.parent()){parent.raw_set(instance.name.as_str(),Value::Nil).map_err(|error|error.to_string())?;}}
                    if dom.get_by_ref(referent).is_some(){
                        let mut stack=vec![referent];let mut subtree=Vec::new();while let Some(item)=stack.pop(){if let Some(instance)=dom.get_by_ref(item){stack.extend_from_slice(instance.children());subtree.push(item);}}
                        if let Some(root_runtime)=self.instances.get(&referent){let mut ancestor=root_runtime.raw_get::<Table>("Parent").ok();if let Some(parent)=ancestor.as_ref(){fire_instance_signal(parent,"ChildRemoved",vec![Value::Table(root_runtime.clone())]).map_err(|error|error.to_string())?;}while let Some(current)=ancestor{for item in &subtree{if let Some(runtime)=self.instances.get(item){fire_instance_signal(&current,"DescendantRemoving",vec![Value::Table(runtime.clone())]).map_err(|error|error.to_string())?;}}ancestor=current.raw_get::<Table>("Parent").ok();}}
                        dom.destroy(referent);destroyed+=subtree.len();
                        for item in subtree {if let Some(runtime)=self.instances.remove(&item){if item!=referent{if let Ok(signal)=runtime.raw_get::<Table>("Destroying"){let fire:Function=signal.get("Fire").map_err(|error|error.to_string())?;fire.call::<()>((signal,Variadic::<Value>::new())).map_err(|error|error.to_string())?;}}runtime.raw_set("Parent",Value::Nil).map_err(|error|error.to_string())?;runtime.raw_set("_destroyed",true).map_err(|error|error.to_string())?;}self.synchronized_properties.remove(&item);}
                    }
                }
                table.raw_set("Parent",Value::Nil).map_err(|error|error.to_string())?;
                self.instances.remove(&referent);self.synchronized_properties.remove(&referent);
            }
        }
        let hierarchy:Vec<(DomRef,String,Option<DomRef>)>=self.instances.iter().filter_map(|(referent,table)|{
            let name=table.raw_get::<String>("Name").ok()?;
            let parent=table.raw_get::<Table>("Parent").ok().and_then(|parent|table_to_ref(&parent).ok().flatten());
            Some((*referent,name,parent))
        }).collect();
        for (referent,name,parent) in hierarchy {
            if referent==dom.root_ref(){continue;}
            let current=dom.get_by_ref(referent).map(|instance|(instance.parent(),instance.name.clone()));
            let Some((current_parent,old_name))=current else{continue;};
            let renamed=old_name!=name;let mut moved=false;
            if renamed {if let Some(instance)=dom.get_by_ref_mut(referent){instance.name=name.clone();}}
            if let Some(parent)=parent {if current_parent!=parent&&dom.get_by_ref(parent).is_some(){dom.transfer_within(referent,parent);moved=true;}}
            if let Some(table)=self.instances.get(&referent) {
                if renamed||moved {
                    let mut moved_runtime=vec![table.clone()];let mut stack=instance_children(table).map_err(|error|error.to_string())?;while let Some(child)=stack.pop(){stack.extend(instance_children(&child).map_err(|error|error.to_string())?);moved_runtime.push(child);}
                    if let Some(old_parent)=self.instances.get(&current_parent){old_parent.raw_set(old_name.as_str(),Value::Nil).map_err(|error|error.to_string())?;if moved{fire_instance_signal(old_parent,"ChildRemoved",vec![Value::Table(table.clone())]).map_err(|error|error.to_string())?;let mut ancestor=Some(old_parent.clone());while let Some(current)=ancestor{for descendant in &moved_runtime{fire_instance_signal(&current,"DescendantRemoving",vec![Value::Table(descendant.clone())]).map_err(|error|error.to_string())?;}ancestor=current.raw_get::<Table>("Parent").ok();}}}
                    let effective_parent=parent.unwrap_or(current_parent);
                    if let Some(new_parent)=self.instances.get(&effective_parent){new_parent.raw_set(name.as_str(),table.clone()).map_err(|error|error.to_string())?;table.raw_set("Parent",new_parent.clone()).map_err(|error|error.to_string())?;if moved{fire_instance_signal(new_parent,"ChildAdded",vec![Value::Table(table.clone())]).map_err(|error|error.to_string())?;let mut ancestor=Some(new_parent.clone());while let Some(current)=ancestor{for descendant in &moved_runtime{fire_instance_signal(&current,"DescendantAdded",vec![Value::Table(descendant.clone())]).map_err(|error|error.to_string())?;}ancestor=current.raw_get::<Table>("Parent").ok();}}}
                }
                if renamed {if let Ok(signal)=table.raw_get::<Table>("Changed"){let fire:Function=signal.get("Fire").map_err(|error|error.to_string())?;fire.call::<()>((signal,"Name")).map_err(|error|error.to_string())?;}}
                if moved {if let Ok(signal)=table.raw_get::<Table>("AncestryChanged"){let fire:Function=signal.get("Fire").map_err(|error|error.to_string())?;fire.call::<()>((signal,table.clone(),table.raw_get::<Value>("Parent").unwrap_or(Value::Nil))).map_err(|error|error.to_string())?;}}
            }
        }
        let mut updates=Vec::new();
        for (referent,names) in &self.synchronized_properties {
            let Some(table)=self.instances.get(referent) else{continue;};
            for name in names {
                let Ok(value)=table.raw_get::<Value>(name.as_str()) else{continue;};
                if value.is_nil(){continue;}
                if let Some(value)=value_to_variant(&self.lua,&value).map_err(|error|error.to_string())? {
                    let existing=dom.get_by_ref(*referent).and_then(|instance|instance.properties.get(&rbx_dom_weak::Ustr::from(name.as_str())));
                    let value=preserve_variant_type(existing,value);
                    let changed=existing.map(|existing|existing!=&value).unwrap_or(true);
                    if changed{updates.push((*referent,name.clone(),value));}
                }
            }
        }
        let count=updates.len()+created+destroyed;
        for (referent,name,value) in updates {if let Some(instance)=dom.get_by_ref_mut(referent){instance.properties.insert(rbx_dom_weak::Ustr::from(name.as_str()),value);}}
        Ok(count)
    }

    pub fn synchronize_from_dom(&self,dom:&WeakDom)->Result<(),String>{
        for (referent,names) in &self.synchronized_properties {
            let (Some(table),Some(instance))=(self.instances.get(referent),dom.get_by_ref(*referent)) else{continue;};
            for name in names {if let Some(value)=instance.properties.get(&rbx_dom_weak::Ustr::from(name.as_str())) { let value=variant_to_value(&self.lua,value).map_err(|error|error.to_string())?;table.raw_set(name.as_str(),value).map_err(|error|error.to_string())?; }}
        }
        Ok(())
    }

    pub fn drain_output(&self)->Vec<OutputLine>{let _=&self.lua;take_log()}
}

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
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(||run_command_inner(dom_rc,source,name)))
        .unwrap_or_else(|panic| {
            let detail=panic.downcast_ref::<&str>().map(|value|(*value).to_string())
                .or_else(||panic.downcast_ref::<String>().cloned()).unwrap_or_else(||"unknown VM panic".into());
            Err(format!("Luau command recovered from an internal error: {detail}"))
        })
}

fn run_command_inner(dom_rc: Rc<RefCell<WeakDom>>, source: &str, name: &str) -> Result<CommandOutcome, String> {
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

/// Recover the DataModel after a command run without assuming every VM-owned
/// handle was released. `Rc::try_unwrap(...).expect(...)` made an otherwise
/// recoverable Lua error crash the whole Android process if a handle survived.
pub fn take_command_dom(dom: Rc<RefCell<WeakDom>>) -> WeakDom {
    match Rc::try_unwrap(dom) {
        Ok(cell) => cell.into_inner(),
        Err(shared) => std::mem::replace(
            &mut *shared.borrow_mut(),
            WeakDom::new(InstanceBuilder::new("DataModel")),
        ),
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
        lua.create_function(move |_lua, (_this, items): (Table, Value)| {
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
        lua.create_function(move |_lua, (_this, item): (Table, Table)| {
            if let Some(r) = table_to_ref(&item)? {
                let mut s = sel.borrow_mut();
                if !s.contains(&r) { s.push(r); }
            }
            Ok(())
        })?
    };
    let sel_remove = {
        let sel = selection.clone();
        lua.create_function(move |_lua, (_this, item): (Table, Table)| {
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
            if is_instance_signal(&key) {
                if let Ok(existing)=this.raw_get::<Table>(&key){return Ok(Value::Table(existing));}
                let signal=make_signal(lua)?; this.raw_set(&key,signal.clone())?;
                return Ok(Value::Table(signal));
            }
            let Some(r) = table_to_ref(&this)? else { return Ok(Value::Nil) };
            enum Resolved { Text(String), Property(DomVariant), Instance(DomRef), Nil }
            let resolved={
                let d=dom.borrow();
                let Some(inst)=d.get_by_ref(r) else{return Ok(Value::Nil);};
                match key.as_str(){
                    "Name"=>Resolved::Text(inst.name.clone()),
                    "ClassName"=>Resolved::Text(inst.class.to_string()),
                    "Parent"=>if inst.parent().is_none(){Resolved::Nil}else{Resolved::Instance(inst.parent())},
                    _=>if let Some(property)=inst.properties.get(&rbx_dom_weak::Ustr::from(key.as_str())){Resolved::Property(property.clone())}
                        else{inst.children().iter().copied().find(|child|d.get_by_ref(*child).is_some_and(|instance|instance.name==key)).map(Resolved::Instance).unwrap_or(Resolved::Nil)},
                }
            };
            Ok(match resolved {
                Resolved::Text(value)=>Value::String(lua.create_string(&value)),
                Resolved::Property(value)=>variant_to_value(lua,&value)?,
                Resolved::Instance(value)=>Value::Table(ref_to_table(lua,dom.clone(),cache.clone(),mt_handle.borrow().as_ref().unwrap().clone(),value)?),
                Resolved::Nil=>Value::Nil,
            })
        })?
    };
    mt.set("__index", index)?;

    // __newindex: Name, Parent, and arbitrary properties.
    let newindex = {
        let dom = dom.clone();
        lua.create_function(move |lua, (this, key, value): (Table, String, Value)| {
            let Some(r) = table_to_ref(&this)? else { return Ok(()) };
            let mut changed=false;
            match key.as_str() {
                "Name" => if let Value::String(s) = value {
                    if let Ok(mut d) = dom.try_borrow_mut() { if let Some(i) = d.get_by_ref_mut(r) { i.name = s.to_str()?; changed=true; } }
                },
                "ClassName" => {} // read-only
                "Parent" => {
                    let new_parent = match value {
                        Value::Table(t) => table_to_ref(&t)?.unwrap_or(dom.borrow().root_ref()),
                        Value::Nil => dom.borrow().root_ref(),
                        _ => return Err(LuaError::runtime("Parent must be an Instance or nil")),
                    };
                    dom.borrow_mut().transfer_within(r, new_parent); changed=true;
                }
                _ => if let Some(variant) = value_to_variant(lua, &value)? {
                    if let Ok(mut d) = dom.try_borrow_mut() {
                        if let Some(i) = d.get_by_ref_mut(r) {
                            i.properties.insert(rbx_dom_weak::Ustr::from(&key), variant);
                            changed=true;
                            COMMAND_OUTCOME.with(|o| o.borrow_mut().mutated += 1);
                        }
                    }
                }
            }
            if changed {
                if let Ok(signal)=this.raw_get::<Table>("Changed") { let fire:Function=signal.get("Fire")?; fire.call::<()>((signal,key.clone()))?; }
                let cache_key=format!("_property_signal_{key}");
                if let Ok(signal)=this.raw_get::<Table>(&cache_key) { let fire:Function=signal.get("Fire")?; fire.call::<()>((signal,))?; }
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
        "GetPropertyChangedSignal" => Some(lua.create_function(|lua,(this,property):(Table,String)|{
            let cache_key=format!("_property_signal_{property}");
            if let Ok(signal)=this.raw_get::<Table>(&cache_key){return Ok(signal);}
            let signal=make_signal(lua)?; this.raw_set(cache_key,signal.clone())?; Ok(signal)
        })?),
        "JumpTo" => Some(lua.create_function(move |_,(layout,page):(Table,Table)|{
            let Some(layout_ref)=table_to_ref(&layout)? else{return Ok(());};
            let Some(page_ref)=table_to_ref(&page)? else{return Ok(());};
            if let Some(instance)=d.borrow_mut().get_by_ref_mut(layout_ref){instance.properties.insert(rbx_dom_weak::Ustr::from("CurrentPage"),DomVariant::Ref(page_ref));COMMAND_OUTCOME.with(|outcome|outcome.borrow_mut().mutated+=1);} Ok(())
        })?),
        "JumpToIndex" => Some(lua.create_function(move |_,(layout,index):(Table,i64)|{
            let Some(layout_ref)=table_to_ref(&layout)? else{return Ok(());};
            let page={let dom=d.borrow();let Some(layout_instance)=dom.get_by_ref(layout_ref)else{return Ok(());};let Some(parent)=dom.get_by_ref(layout_instance.parent())else{return Ok(());};let mut pages:Vec<DomRef>=parent.children().iter().copied().filter(|child|*child!=layout_ref).collect();pages.sort_by_key(|child|dom.get_by_ref(*child).and_then(|instance|instance.properties.get(&rbx_dom_weak::ustr("LayoutOrder"))).and_then(|value|match value{DomVariant::Int32(value)=>Some(*value),DomVariant::Int64(value)=>Some(*value as i32),_=>None}).unwrap_or(0));pages.get(index.max(0) as usize).copied()};
            if let Some(page)=page{if let Some(instance)=d.borrow_mut().get_by_ref_mut(layout_ref){instance.properties.insert(rbx_dom_weak::Ustr::from("CurrentPage"),DomVariant::Ref(page));COMMAND_OUTCOME.with(|outcome|outcome.borrow_mut().mutated+=1);}} Ok(())
        })?),
        "Next" | "Previous" => {
            let step=if name=="Next"{1isize}else{-1isize};
            Some(lua.create_function(move |_,layout:Table|{
                let Some(layout_ref)=table_to_ref(&layout)? else{return Ok(());};
                let page={let dom=d.borrow();let Some(layout_instance)=dom.get_by_ref(layout_ref)else{return Ok(());};let current=match layout_instance.properties.get(&rbx_dom_weak::ustr("CurrentPage")){Some(DomVariant::Ref(value))=>Some(*value),_=>None};let Some(parent)=dom.get_by_ref(layout_instance.parent())else{return Ok(());};let mut pages:Vec<DomRef>=parent.children().iter().copied().filter(|child|*child!=layout_ref).collect();pages.sort_by_key(|child|dom.get_by_ref(*child).and_then(|instance|instance.properties.get(&rbx_dom_weak::ustr("LayoutOrder"))).and_then(|value|match value{DomVariant::Int32(value)=>Some(*value),DomVariant::Int64(value)=>Some(*value as i32),_=>None}).unwrap_or(0));if pages.is_empty(){None}else{let current_index=current.and_then(|value|pages.iter().position(|page|*page==value)).unwrap_or(0)as isize;Some(pages[(current_index+step).clamp(0,pages.len()as isize-1)as usize])}};
                if let Some(page)=page{if let Some(instance)=d.borrow_mut().get_by_ref_mut(layout_ref){instance.properties.insert(rbx_dom_weak::Ustr::from("CurrentPage"),DomVariant::Ref(page));COMMAND_OUTCOME.with(|outcome|outcome.borrow_mut().mutated+=1);}} Ok(())
            })?)
        },
        "TweenPosition" | "TweenSize" => {
            let property=if name=="TweenPosition"{"Position"}else{"Size"}.to_string();
            Some(lua.create_function(move |lua,(this,args):(Table,Variadic<Value>)|{
                let Some(referent)=table_to_ref(&this)? else{return Ok(false);};
                let Some(first)=args.first() else{return Ok(false);};
                let Some(value)=value_to_variant(lua,first)? else{return Ok(false);};
                if let Some(instance)=d.borrow_mut().get_by_ref_mut(referent){instance.properties.insert(rbx_dom_weak::Ustr::from(property.as_str()),value);COMMAND_OUTCOME.with(|outcome|outcome.borrow_mut().mutated+=1);}
                if let Some(Value::Function(callback))=args.last(){callback.call::<()>(())?;} Ok(true)
            })?)
        },
        "TweenSizeAndPosition" => Some(lua.create_function(move |lua,(this,args):(Table,Variadic<Value>)|{
            let Some(referent)=table_to_ref(&this)? else{return Ok(false);};
            let size=args.first().map(|value|value_to_variant(lua,value)).transpose()?.flatten();
            let position=args.get(1).map(|value|value_to_variant(lua,value)).transpose()?.flatten();
            if let Some(instance)=d.borrow_mut().get_by_ref_mut(referent){if let Some(value)=size{instance.properties.insert(rbx_dom_weak::Ustr::from("Size"),value);}if let Some(value)=position{instance.properties.insert(rbx_dom_weak::Ustr::from("Position"),value);}COMMAND_OUTCOME.with(|outcome|outcome.borrow_mut().mutated+=1);}
            if let Some(Value::Function(callback))=args.last(){callback.call::<()>(())?;} Ok(true)
        })?),
        "Create" => Some(lua.create_function(move |lua, (service, target, _info, goals): (Table, Table, Value, Table)| {
            let class:String=service.raw_get("_class").unwrap_or_default();
            if class!="TweenService" { return Err(LuaError::runtime("Create is only available on TweenService")); }
            let target_ref=table_to_ref(&target)?;
            let tween=lua.create_table();
            let completed=make_signal(lua)?;
            tween.set("Completed",completed.clone())?;
            let play_dom=d.clone(); let play_goals=goals.clone();
            let play_completed=completed.clone();
            tween.set("Play",lua.create_function(move |lua,_tween:Table|{
                if let Some(referent)=target_ref {
                    let mut updates=Vec::new();
                    for pair in play_goals.clone().pairs::<Value,Value>() {
                        let (key,value)=pair?;
                        if let Value::String(key)=key { if let Some(value)=value_to_variant(lua,&value)? { updates.push((key.to_str()?,value)); } }
                    }
                    if let Some(instance)=play_dom.borrow_mut().get_by_ref_mut(referent) {
                        for (key,value) in updates { instance.properties.insert(rbx_dom_weak::Ustr::from(key.as_str()),value); COMMAND_OUTCOME.with(|outcome|outcome.borrow_mut().mutated+=1); }
                    }
                }
                let fire:Function=play_completed.get("Fire")?;
                fire.call::<()>((play_completed.clone(),Variadic::<Value>::new()))
            })?)?;
            tween.set("Pause",lua.create_function(|_,_tween:Table|Ok(()))?)?;
            tween.set("Cancel",lua.create_function(|_,_tween:Table|Ok(()))?)?;
            Ok(tween)
        })?),
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
    // Keep this lookup inside one immutable borrow. Borrowing `dom` again from
    // the iterator closure used to panic (`RefCell already borrowed`) as soon
    // as command mode tried to resolve Workspace, aborting the Android app.
    let (root, existing) = {
        let d = dom.borrow();
        let root = d.root_ref();
        let existing = d.get_by_ref(root).and_then(|root_inst| {
            root_inst.children().iter().copied().find(|c| {
                d.get_by_ref(*c)
                    .is_some_and(|i| i.class == name || i.name == name)
            })
        });
        (root, existing)
    };
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
        Variant::Vector2(v) => {
            let t=lua.create_table();t.set("X",v.x as f64)?;t.set("Y",v.y as f64)?;Value::Table(t)
        }
        Variant::UDim(v) => {
            let t=lua.create_table();t.set("Scale",v.scale as f64)?;t.set("Offset",v.offset as i64)?;Value::Table(t)
        }
        Variant::UDim2(v) => {
            let t=lua.create_table();t.set("XScale",v.x.scale as f64)?;t.set("XOffset",v.x.offset as i64)?;t.set("YScale",v.y.scale as f64)?;t.set("YOffset",v.y.offset as i64)?;Value::Table(t)
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
            if has("XScale") && has("XOffset") && has("YScale") && has("YOffset") {
                Some(DomVariant::UDim2(ty::UDim2::new(
                    ty::UDim::new(t.get::<f64>("XScale")? as f32,t.get::<i64>("XOffset")? as i32),
                    ty::UDim::new(t.get::<f64>("YScale")? as f32,t.get::<i64>("YOffset")? as i32))))
            } else if has("Scale") && has("Offset") {
                Some(DomVariant::UDim(ty::UDim::new(t.get::<f64>("Scale")? as f32,t.get::<i64>("Offset")? as i32)))
            } else if has("EnumType") && has("Value") {
                Some(DomVariant::Enum(ty::Enum::from_u32(t.get::<i64>("Value")?.max(0) as u32)))
            } else if has("R") && has("G") && has("B") {
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
            } else if has("X") && has("Y") {
                Some(DomVariant::Vector2(ty::Vector2::new(t.get::<f64>("X")? as f32,t.get::<f64>("Y")? as f32)))
            } else {
                None
            }
        }
        _ => None,
    })
}
