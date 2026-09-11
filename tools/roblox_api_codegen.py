#!/usr/bin/env python3
"""Automatic machine: builds `src/roblox_api_data.rs` for Luau autocomplete.

Sources (all optional except the user's dump):

1. dump/roblox_builtins_only.json            (required)
     Roblox "builtins-only" API dump uploaded by the user: instance classes
     and their script-usable Functions (names, parameters, return types).

2. dump/cache/API-Dump.json                  (traced from maximum adhd's client
                                             tracker; `--fetch` downloads it)
     Modern Roblox API dump: every class, superclass, and typed Members
     (Property/Function/Event/Callback) with ValueType/ReturnType names.

3. dump/cache/api-docs-en-us.json            (`--fetch`)
     Roblox API documentation tree. Supplies DATATYPES (Vector3, CFrame,
     Color3, ...), their members, and the static-vs-method classification
     (`@roblox/global/<T>.<m>` = dot access, `@roblox/globaltype/<T>.<m>`
     with a `self` parameter = colon method, no params = property).

The script merges 1-3 and emits src/roblox_api_data.rs with:

  - CLASSES   : instance classes + datatypes (name, superclass, members)
  - ENUMS     : enum name -> items (for `Enum.KeyCode.Item` completion)
  - LIBRARIES : Luau built-in libraries (task, math, string, table, ...)
  - GLOBAL_TYPES / BARE_GLOBALS : `game`, `script`, `Vector3`, `task`, ...

Member kinds: 0 = property (`x.Parent`), 1 = method (`x:Destroy()`),
2 = static function (`Vector3.new()`), 3 = event (`x.CharacterAdded`),
4 = callback (`x.SomeCallback`), 5 = field.

Usage:
    python3 tools/roblox_api_codegen.py            # generate from dump/
    python3 tools/roblox_api_codegen.py --fetch    # also refresh dump/cache/
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
USER_DUMP = ROOT / "dump" / "roblox_builtins_only.json"
CACHE = ROOT / "dump" / "cache"
TRACKER_DUMP = CACHE / "API-Dump.json"
API_DOCS = CACHE / "api-docs-en-us.json"
OUTPUT = ROOT / "src" / "roblox_api_data.rs"

TRACKER_REPO = "https://github.com/MaximumADHD/Roblox-Client-Tracker.git"

KIND_PROPERTY = 0
KIND_METHOD = 1
KIND_STATIC = 2
KIND_EVENT = 3
KIND_CALLBACK = 4
KIND_FIELD = 5

# --------------------------------------------------------------------------
# Static classification of *built-in* Luau libraries. These are stock Luau
# (not Roblox), so they are stable enough to keep as literal data here.
# --------------------------------------------------------------------------
LUAU_LIBRARIES: dict[str, list[tuple[str, str, int]]] = {
    "task": [
        ("cancel", "task.cancel(thread)", KIND_STATIC),
        ("defer", "task.defer(threadOrFunction, ...)", KIND_STATIC),
        ("delay", "task.delay(duration, threadOrFunction, ...)", KIND_STATIC),
        ("desynchronize", "task.desynchronize()", KIND_STATIC),
        ("spawn", "task.spawn(threadOrFunction, ...)", KIND_STATIC),
        ("synchronize", "task.synchronize()", KIND_STATIC),
        ("wait", "task.wait(duration?)  → number", KIND_STATIC),
    ],
    "math": [
        ("abs", "math.abs(x)", KIND_STATIC), ("acos", "math.acos(x)", KIND_STATIC),
        ("asin", "math.asin(x)", KIND_STATIC), ("atan", "math.atan(y, x?)", KIND_STATIC),
        ("atan2", "math.atan2(y, x)", KIND_STATIC), ("ceil", "math.ceil(x)", KIND_STATIC),
        ("clamp", "math.clamp(x, min, max)", KIND_STATIC), ("cos", "math.cos(x)", KIND_STATIC),
        ("deg", "math.deg(rad)", KIND_STATIC), ("exp", "math.exp(x)", KIND_STATIC),
        ("floor", "math.floor(x)", KIND_STATIC), ("fmod", "math.fmod(a, b)", KIND_STATIC),
        ("frexp", "math.frexp(x)", KIND_STATIC), ("huge", "math.huge", KIND_PROPERTY),
        ("ldexp", "math.ldexp(x, exp)", KIND_STATIC), ("log", "math.log(x, base?)", KIND_STATIC),
        ("max", "math.max(...)", KIND_STATIC), ("min", "math.min(...)", KIND_STATIC),
        ("modf", "math.modf(x)", KIND_STATIC), ("pi", "math.pi", KIND_PROPERTY),
        ("rad", "math.rad(deg)", KIND_STATIC), ("random", "math.random(m?, n?)", KIND_STATIC),
        ("randomseed", "math.randomseed(x?, y?)", KIND_STATIC), ("sign", "math.sign(x)", KIND_STATIC),
        ("sin", "math.sin(x)", KIND_STATIC), ("sqrt", "math.sqrt(x)", KIND_STATIC),
        ("tan", "math.tan(x)", KIND_STATIC), ("tointeger", "math.tointeger(x)", KIND_STATIC),
        ("type", "math.type(x)", KIND_STATIC), ("ultrandom", "math.ultrandom()", KIND_STATIC),
    ],
    "string": [
        ("byte", "string.byte(s, i?, j?)", KIND_STATIC), ("char", "string.char(...)", KIND_STATIC),
        ("find", "string.find(s, pattern, init?, plain?)", KIND_STATIC), ("format", "string.format(s, ...)", KIND_STATIC),
        ("gmatch", "string.gmatch(s, pattern)", KIND_STATIC), ("gsub", "string.gsub(s, pattern, repl, n?)", KIND_STATIC),
        ("len", "string.len(s)", KIND_STATIC), ("lower", "string.lower(s)", KIND_STATIC),
        ("match", "string.match(s, pattern, init?)", KIND_STATIC), ("pack", "string.pack(fmt, ...)", KIND_STATIC),
        ("packsize", "string.packsize(fmt)", KIND_STATIC), ("rep", "string.rep(s, n, sep?)", KIND_STATIC),
        ("reverse", "string.reverse(s)", KIND_STATIC), ("split", "string.split(s, sep?, plain?)", KIND_STATIC),
        ("sub", "string.sub(s, i, j?)", KIND_STATIC), ("unpack", "string.unpack(fmt, s, pos?)", KIND_STATIC),
        ("upper", "string.upper(s)", KIND_STATIC),
    ],
    "table": [
        ("clear", "table.clear(t)", KIND_STATIC), ("clone", "table.clone(t)", KIND_STATIC),
        ("concat", "table.concat(t, sep?, i?, j?)", KIND_STATIC), ("create", "table.create(n, value?)", KIND_STATIC),
        ("find", "table.find(t, value, init?)", KIND_STATIC), ("foreach", "table.foreach(t, f)", KIND_STATIC),
        ("foreachi", "table.foreachi(t, f)", KIND_STATIC), ("freeze", "table.freeze(t)", KIND_STATIC),
        ("getn", "table.getn(t)", KIND_STATIC), ("insert", "table.insert(t, pos?, value)", KIND_STATIC),
        ("isfrozen", "table.isfrozen(t)", KIND_STATIC), ("maxn", "table.maxn(t)", KIND_STATIC),
        ("move", "table.move(a1, f, e, t, a2?)", KIND_STATIC), ("pack", "table.pack(...)", KIND_STATIC),
        ("remove", "table.remove(t, pos?)", KIND_STATIC), ("sort", "table.sort(t, comp?)", KIND_STATIC),
        ("unpack", "table.unpack(t, i?, j?)", KIND_STATIC),
    ],
    "os": [
        ("clock", "os.clock()", KIND_STATIC), ("date", "os.date(format?, time?)", KIND_STATIC),
        ("difftime", "os.difftime(t2, t1)", KIND_STATIC), ("time", "os.time(table?)", KIND_STATIC),
    ],
    "coroutine": [
        ("close", "coroutine.close(co)", KIND_STATIC), ("create", "coroutine.create(f)", KIND_STATIC),
        ("isyieldable", "coroutine.isyieldable(co?)", KIND_STATIC), ("resume", "coroutine.resume(co, ...)", KIND_STATIC),
        ("running", "coroutine.running()", KIND_STATIC), ("status", "coroutine.status(co)", KIND_STATIC),
        ("wrap", "coroutine.wrap(f)", KIND_STATIC), ("yield", "coroutine.yield(...)", KIND_STATIC),
    ],
    "utf8": [
        ("char", "utf8.char(...)", KIND_STATIC), ("charpattern", "utf8.charpattern", KIND_PROPERTY),
        ("codes", "utf8.codes(s)", KIND_STATIC), ("codepoint", "utf8.codepoint(s, i?, j?)", KIND_STATIC),
        ("len", "utf8.len(s, i?, j?)", KIND_STATIC), ("offset", "utf8.offset(s, n, i?)", KIND_STATIC),
    ],
    "bit32": [
        ("arshift", "bit32.arshift(x, disp)", KIND_STATIC), ("band", "bit32.band(...)", KIND_STATIC),
        ("bnot", "bit32.bnot(x)", KIND_STATIC), ("bor", "bit32.bor(...)", KIND_STATIC),
        ("btest", "bit32.btest(...)", KIND_STATIC), ("bxor", "bit32.bxor(...)", KIND_STATIC),
        ("extract", "bit32.extract(n, field, width?)", KIND_STATIC), ("lrotate", "bit32.lrotate(x, disp)", KIND_STATIC),
        ("lshift", "bit32.lshift(x, disp)", KIND_STATIC), ("replace", "bit32.replace(n, v, field, width?)", KIND_STATIC),
        ("rrotate", "bit32.rrotate(x, disp)", KIND_STATIC), ("rshift", "bit32.rshift(x, disp)", KIND_STATIC),
    ],
    "buffer": [
        ("copy", "buffer.copy(target, targetOffset, source, sourceOffset?, count?)", KIND_STATIC),
        ("create", "buffer.create(size)", KIND_STATIC), ("fromstring", "buffer.fromstring(str)", KIND_STATIC),
        ("len", "buffer.len(buf)", KIND_STATIC), ("readbits", "buffer.readbits(buf, offset, bitCount)", KIND_STATIC),
        ("readbit", "buffer.readbit(buf, offset)", KIND_STATIC),
        ("readf32", "buffer.readf32(buf, offset)", KIND_STATIC), ("readf64", "buffer.readf64(buf, offset)", KIND_STATIC),
        ("readi8", "buffer.readi8(buf, offset)", KIND_STATIC), ("readi16", "buffer.readi16(buf, offset)", KIND_STATIC),
        ("readi32", "buffer.readi32(buf, offset)", KIND_STATIC), ("readu8", "buffer.readu8(buf, offset)", KIND_STATIC),
        ("readu16", "buffer.readu16(buf, offset)", KIND_STATIC), ("readu32", "buffer.readu32(buf, offset)", KIND_STATIC),
        ("readstring", "buffer.readstring(buf, offset, count)", KIND_STATIC),
        ("tostring", "buffer.tostring(buf)", KIND_STATIC), ("writebits", "buffer.writebits(buf, offset, value, bitCount)", KIND_STATIC),
        ("writebit", "buffer.writebit(buf, offset, value)", KIND_STATIC), ("writef32", "buffer.writef32(buf, offset, value)", KIND_STATIC),
        ("writef64", "buffer.writef64(buf, offset, value)", KIND_STATIC), ("writei8", "buffer.writei8(buf, offset, value)", KIND_STATIC),
        ("writei16", "buffer.writei16(buf, offset, value)", KIND_STATIC), ("writei32", "buffer.writei32(buf, offset, value)", KIND_STATIC),
        ("writeu8", "buffer.writeu8(buf, offset, value)", KIND_STATIC), ("writeu16", "buffer.writeu16(buf, offset, value)", KIND_STATIC),
        ("writeu32", "buffer.writeu32(buf, offset, value)", KIND_STATIC), ("writestring", "buffer.writestring(buf, offset, value)", KIND_STATIC),
    ],
}

# Label -> (detail, insertion text or None). Stock Luau functions that live in
# no table (called bare) + Roblox globals. Datatypes get appended automatically.
BARE_GLOBALS: list[tuple[str, str, str | None]] = [
    ("assert", "Luau builtin", "assert()"),
    ("collectgarbage", "Luau GC control", "collectgarbage()"),
    ("delay", "Delay a callback by seconds", "delay(delayTime, callback)"),
    ("elapsedTime", "Time since task start", "elapsedTime()"),
    ("error", "Raise a runtime error", "error()"),
    ("game", "Roblox DataModel (service provider)", None),
    ("getmetatable", "Luau builtin", "getmetatable()"),
    ("gcinfo", "Luau GC info", "gcinfo()"),
    ("Instance", "Roblox global constructor table", None),
    ("ipairs", "Iterate an array", "ipairs()"),
    ("loadstring", "Compile a string of code", "loadstring()"),
    ("newproxy", "Luau builtin", "newproxy()"),
    ("next", "Luau builtin", "next()"),
    ("pairs", "Iterate a table", "pairs()"),
    ("pairs", "Iterate a table", "pairs()"),
    ("pcall", "Protected call", "pcall()"),
    ("plugin", "Studio plugin object (Studio only)", None),
    ("print", "Write to Output", "print()"),
    ("printidentity", "Debug print identity", "printidentity()"),
    ("rawequal", "Luau builtin", "rawequal()"),
    ("rawget", "Luau builtin", "rawget()"),
    ("rawset", "Luau builtin", "rawset()"),
    ("require", "Load a ModuleScript", "require()"),
    ("select", "Luau builtin", "select()"),
    ("setmetatable", "Luau builtin", "setmetatable()"),
    ("shared", "Roblox shared table (Studio)", None),
    ("spawn", "Spawn a callback in a new thread", "spawn(callback)"),
    ("stats", "Roblox engine statistics", "stats()"),
    ("task", "Luau task scheduler library", None),
    ("tick", "Unix time in seconds", "tick()"),
    ("time", "Engine time in seconds", "time()"),
    ("tonumber", "Luau builtin", "tonumber()"),
    ("tostring", "Luau builtin", "tostring()"),
    ("type", "Luau builtin", "type()"),
    ("typeof", "Roblox type of a value", "typeof()"),
    ("unpack", "Luau builtin", "unpack()"),
    ("warn", "Write a warning to Output", "warn()"),
    ("wait", "Yield the current thread", "wait()"),
    ("workspace", "Roblox Workspace service", None),
    ("xpcall", "Protected call with handler", "xpcall()"),
]

# Global value -> API type used for member completion & type inference.
GLOBAL_TYPES: dict[str, str] = {
    "game": "DataModel",
    "workspace": "Workspace",
    "script": "Script",
    "plugin": "Plugin",
    "Instance": "Instance",
    "Enum": "@enum",
}


# --------------------------------------------------------------------------
# Fetching (--fetch)
# --------------------------------------------------------------------------
def fetch_sources() -> None:
    CACHE.mkdir(parents=True, exist_ok=True)
    if not (TRACKER_DUMP.exists() and API_DOCS.exists()):
        tmp = CACHE / "tracker"
        if not (tmp / ".git").exists():
            print(f"[fetch] {TRACKER_REPO} (sparse, branch roblox) ...")
            subprocess.run(
                ["git", "clone", "--depth", "1", "--filter=blob:none", "--sparse",
                 "--branch", "roblox", TRACKER_REPO, str(tmp)],
                check=True,
            )
        subprocess.run(["git", "-C", str(tmp), "sparse-checkout", "set",
                        "--skip-checks", "API-Dump.json", "api-docs/en-us.json"], check=True)
        (tmp / "API-Dump.json").rename(TRACKER_DUMP)
        (tmp / "api-docs" / "en-us.json").rename(API_DOCS)


# --------------------------------------------------------------------------
# Sources
# --------------------------------------------------------------------------
def load_user_dump() -> dict:
    with open(USER_DUMP, encoding="utf-8") as fh:
        data = json.load(fh)
    classes: dict[str, dict] = {}
    for cls in data.get("Classes", []):
        name = cls.get("ClassName")
        if name:
            classes[name] = cls
    return {"classes": classes, "version": data.get("Version", "unknown")}


def load_tracker_dump() -> dict:
    if not TRACKER_DUMP.exists():
        return {"classes": {}, "enums": {}, "version": None}
    with open(TRACKER_DUMP, encoding="utf-8") as fh:
        data = json.load(fh)
    classes: dict[str, dict] = {}
    for cls in data.get("Classes", []):
        name = cls.get("Name") or cls.get("ClassName")
        if name:
            classes[name] = cls
    enums: dict[str, list[tuple[str, int]]] = {}
    for enum in data.get("Enums", []):
        name = enum.get("Name")
        if name:
            enums[name] = [(item["Name"], item.get("Value", 0)) for item in enum.get("Items", [])]
    return {"classes": classes, "enums": enums, "version": data.get("Version")}


def load_api_docs() -> dict | None:
    if not API_DOCS.exists():
        return None
    with open(API_DOCS, encoding="utf-8") as fh:
        return json.load(fh)


# --------------------------------------------------------------------------
# Type helpers
# --------------------------------------------------------------------------
def type_word(type_info: dict | list | None) -> str:
    """Render a single type descriptor or a multi-return list as a word."""
    if not type_info:
        return "?"
    if isinstance(type_info, list):
        words = [type_word(entry) for entry in type_info if entry]
        return ", ".join(words) if words else "?"
    category = type_info.get("Category", "Primitive")
    name = type_info.get("Name", "?")
    if category == "Primitive":
        return name.lower()
    if category == "Group":
        return str(name).lower()
    return str(name)


def type_api_name(type_info: dict | list | None) -> str:
    """API type used for onward type-flow, or '' for primitives/unknowns."""
    if not type_info:
        return ""
    entries = type_info if isinstance(type_info, list) else [type_info]
    for entry in entries:
        if not entry:
            continue
        category = entry.get("Category", "")
        name = entry.get("Name", "")
        if category in ("Class", "DataType"):
            return str(name)
        if category == "Enum":
            return f"Enum:{name}"
    return ""


def param_text(parameters: list[dict] | None) -> str:
    out = []
    for param in parameters or []:
        name = str(param.get("Name") or param.get("name") or "?")
        if param.get("Default") is not None:
            name += "?"
        t = type_word(param.get("Type"))
        out.append(f"{name}: {t}" if t != "?" else name)
    return ", ".join(out)


def function_detail(name: str, parameters: list[dict] | None,
                    return_type: dict | None, kind: int) -> str:
    ret = type_word(return_type)
    ret_suffix = f" → {ret}" if ret and ret != "null" else ""
    prefix = "static " if kind == KIND_STATIC else ""
    return f"{prefix}{name}({param_text(parameters)}){ret_suffix}"


# --------------------------------------------------------------------------
# Datatypes from the documentation tree
# --------------------------------------------------------------------------
def doc_member_kind(entry: dict) -> int:
    params = entry.get("params") or []
    if params:
        first = (params[0].get("name") or "").lower()
        return KIND_METHOD if first == "self" else KIND_STATIC
    return KIND_PROPERTY


def overload_signature(entry: dict) -> str | None:
    """Signatures look like `@roblox/global/CFrame.new/overload/(Vector3) -> CFrame`.

    The overload key itself carries the typed signature; use it when present.
    """
    if "keys" in entry:
        for key in entry["keys"].values():
            if "/overload/" in key:
                return key.rsplit("/", 1)[-1]
    return None


def collect_datatypes(docs: dict | None, tracker_classes: dict[str, dict]) -> dict[str, dict]:
    """-> {typ: {'members': [(name, detail, kind, api_type)], 'statics': [...]}}"""
    out: dict[str, dict] = defaultdict(lambda: {"members": {}, "statics": {}})
    if not docs:
        return out
    for key, entry in docs.items():
        parts = key.split("/")
        if len(parts) < 3 or parts[1] not in ("global", "globaltype"):
            continue
        rest = "/".join(parts[2:])
        if "." not in rest:
            continue
        typ, member = rest.split(".", 1)
        if "." in member or "/" in member:
            continue  # nested (X.Y.Z) or params/overload entries handled separately
        if not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", member):
            continue  # doc-tree gunk ("function" Color, Studio keys, ...)
        kind = doc_member_kind(entry)
        params = entry.get("params") or []
        # `self` names the receiver, not an argument; hide it in the detail.
        visible_params = [p for p in params if (p.get("name") or "").lower() != "self"]
        if parts[1] == "global":
            # Static constructor tables (Instance.new, Vector3.new, ...).
            detail = member if kind == KIND_PROPERTY else f"{member}({param_text(visible_params)})"
            out[typ]["statics"][member] = (member, detail, kind, "")
        elif typ not in tracker_classes:
            # Instance classes come from the structured dump; only datatypes
            # (Vector3, CFrame, RBXScriptSignal, ...) use the docs here.
            detail = member if kind == KIND_PROPERTY else f"{member}({param_text(visible_params)})"
            out[typ]["members"][member] = (member, detail, kind, "")
    return out


# --------------------------------------------------------------------------
# Main generation
# --------------------------------------------------------------------------
def build_model(user: dict, tracker: dict, docs: dict | None):
    tracker_classes = tracker["classes"]
    class_sources = dict(tracker_classes)
    # Union with the user's dump (authoritative for script-usable functions).
    for name, cls in user["classes"].items():
        class_sources.setdefault(name, {})

    datatypes = collect_datatypes(docs, tracker_classes)

    # --- instance classes -------------------------------------------------
    classes: dict[str, dict] = {}
    for name, cls in class_sources.items():
        superclass = cls.get("Superclass") or cls.get("superclass") or ""
        if superclass == "<<<ROOT>>>":
            superclass = ""
        members: dict[str, tuple[str, str, int, str]] = {}

        # Functions: user dump first (script-usable), tracker fills gaps.
        function_sources = []
        if name in user["classes"] and "Functions" in user["classes"][name]:
            function_sources.append(("user", user["classes"][name]["Functions"]))
        if "Members" in cls:
            tracker_fns = [m for m in cls["Members"] if m.get("MemberType") == "Function"]
            function_sources.append(("tracker", tracker_fns))

        for source, fns in function_sources:
            for fn in fns:
                fname = fn.get("Name")
                if not fname or not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", fname) or fname in members:
                    continue
                # Static vs method: the docs put statics under @roblox/global.
                is_static = False
                if docs:
                    is_static = f"@roblox/global/{name}.{fname}" in docs
                kind = KIND_STATIC if is_static else KIND_METHOD
                detail = function_detail(fname, fn.get("Parameters"),
                                         fn.get("ReturnType"), kind)
                api_type = type_api_name(fn.get("ReturnType"))
                members[fname] = (fname, detail, kind, api_type)

        # Properties, events, callbacks (tracker dump).
        for member in cls.get("Members", []):
            mtype = member.get("MemberType")
            mname = member.get("Name")
            if not mname or not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", mname) or mname in members:
                continue
            if mtype == "Property":
                value = member.get("ValueType") or {}
                t = type_word(value)
                detail = f"property {mname}: {t}" if t != "?" else f"property {mname}"
                members[mname] = (mname, detail, KIND_PROPERTY, type_api_name(value))
            elif mtype == "Event":
                members[mname] = (mname, f"event {mname}", KIND_EVENT, "RBXScriptSignal")
            elif mtype == "Callback":
                detail = f"callback {mname}({param_text(member.get('Parameters'))})"
                members[mname] = (mname, detail, KIND_CALLBACK, "")

        # Statics for classes with constructor tables (Instance.new,
        # game.GetService is a method; Color3.fromRGB is a static, ...).
        for record in datatypes[name]["statics"].values():
            members.setdefault(record[0], record)
        # Datatype entries for class-named things the docs classify as such
        # (e.g. RBXScriptSignal is not in the structured dump).
        if name in datatypes:
            for record in datatypes[name]["members"].values():
                members.setdefault(record[0], record)

        classes[name] = {
            "superclass": superclass,
            "datatype": name in datatypes,
            "members": members,
        }

    # --- pure datatypes + their statics ------------------------------------
    for typ, data in datatypes.items():
        if typ in classes:
            continue
        members = dict(data["members"])
        members.update(data["statics"])
        classes[typ] = {"superclass": "", "datatype": True, "members": members}

    # --- enums -------------------------------------------------------------
    enums = tracker.get("enums", {})

    return {"classes": classes, "enums": enums, "datatypes": datatypes}


# --------------------------------------------------------------------------
# Rust emission
# --------------------------------------------------------------------------
HEADER = """//! GENERATED FILE — DO NOT EDIT. Built by tools/roblox_api_codegen.py.
//!
//! Sources:
//!   dump/roblox_builtins_only.json (uploaded Roblox builtins API dump)
//!   Roblox-Client-Tracker API-Dump.json + api-docs (see dump/cache/)
//!
//! Regenerate: python3 tools/roblox_api_codegen.py [--fetch]
//!
//! Member kinds: 0 property (dot) · 1 method (colon) · 2 static (dot)
//!               3 event (dot) · 4 callback (dot)

#![allow(dead_code)]

/// (name, detail, kind, api_type · '' = primitive/unknown · \"Enum:X\" = enum)
pub type ApiMember = (&'static str, &'static str, u8, &'static str);

pub struct ApiClass {
    pub name: &'static str,
    pub superclass: &'static str,
    pub datatype: bool,
    pub members: &'static [ApiMember],
}

pub static CLASSES: &[ApiClass] = &[
"""

FOOTER_TEMPLATE = """
];

pub static ENUMS: &[(&'static str, &'static [&'static str])] = &[
{enums}
];

pub static LIBRARIES: &[(&'static str, &'static [ApiMember])] = &[
{libraries}
];

/// Global value -> API type used for member completion/type inference.
/// \"@enum\" and \"@lib:<name>\" are handled specially by the completion engine.
pub static GLOBAL_TYPES: &[(&'static str, &'static str)] = &[
{global_types}
];

/// Bare identifiers that are always in scope (stock Luau + Roblox globals +
/// datatypes), label -> (detail, insertion_text?).
pub static BARE_GLOBALS: &[(&'static str, &'static str, &'static str)] = &[
{bare_globals}
];

fn escape(value: &str) -> String {{
    value.replace('\\\\', "\\\\\\\\").replace('"', "\\\\\\"")
}}

fn cmp_class(lhs: &&ApiClass, rhs: &&ApiClass) -> std::cmp::Ordering {{
    lhs.name.cmp(rhs.name)
}}

/// Case-sensitive class/datatype lookup.
pub fn find_class(name: &str) -> Option<&'static ApiClass> {{
    CLASSES.binary_search_by(|class| class.name.as_bytes().cmp(name.as_bytes()))
        .ok()
        .map(|index| &CLASSES[index])
}}

pub fn class_superclass(name: &str) -> Option<&'static str> {{
    find_class(name).map(|class| class.superclass)
}}

/// Whether `name` is a class, datatype, or enum that can be used as a typed
/// annotation or inferred for member completion.
pub fn is_api_type(name: &str) -> bool {{
    find_class(name).is_some() || name.starts_with("Enum:") || enum_items(name).is_some()
}}

pub fn library(name: &str) -> Option<&'static [ApiMember]> {{
    LIBRARIES
        .iter()
        .find(|(label, _)| *label == name)
        .map(|(_, members)| *members)
}}

pub fn global_type(name: &str) -> Option<&'static str> {{
    GLOBAL_TYPES
        .iter()
        .find(|(label, _)| *label == name)
        .map(|(_, ty)| *ty)
}}

pub fn enum_items(name: &str) -> Option<&'static [&'static str]> {{
    ENUMS
        .iter()
        .find(|(label, _)| *label == name)
        .map(|(_, items)| *items)
}}

/// Members of `owner` including inherited ones, as (name, detail, kind, type).
pub fn members_of(owner: &str) -> Vec<ApiMember> {{
    let mut out = Vec::new();
    let mut current = Some(owner);
    let mut guard = 0;
    while let Some(class_name) = current {{
        guard += 1;
        if guard > 64 {{
            break;
        }}
        if let Some(class) = find_class(class_name) {{
            out.extend_from_slice(class.members);
        }}
        current = find_class(class_name)
            .map(|class| class.superclass)
            .filter(|superclass| !superclass.is_empty());
    }}
    out
}}

/// The API type of `member` on `owner` (walking inheritance), for type flow.
pub fn member_type(owner: &str, member: &str) -> Option<&'static str> {{
    let mut current = Some(owner);
    let mut guard = 0;
    while let Some(class_name) = current {{
        guard += 1;
        if guard > 64 {{
            break;
        }}
        if let Some(class) = find_class(class_name) {{
            if let Some((_, _, _, api_type)) = class.members.iter().find(|(name, _, _, _)| *name == member) {{
                if api_type.is_empty() {{
                    return None;
                }}
                return Some(api_type);
            }}
        }}
        current = find_class(class_name)
            .map(|class| class.superclass)
            .filter(|superclass| !superclass.is_empty());
    }}
    None
}}

/// Whether `kind` is reachable through `separator` (`.` or `:`). Roblox
/// convention: properties/events/statics use dot, methods use colon.
pub fn kind_matches_separator(kind: u8, separator: char) -> bool {{
    match separator {{
        ':' => kind == KIND_METHOD,
        _ => kind != KIND_METHOD,
    }}
}}

pub const KIND_PROPERTY: u8 = 0;
pub const KIND_METHOD: u8 = 1;
pub const KIND_STATIC: u8 = 2;
pub const KIND_EVENT: u8 = 3;
pub const KIND_CALLBACK: u8 = 4;
pub const KIND_FIELD: u8 = 5;
"""


def render_member(record: tuple[str, str, int, str]) -> str:
    name, detail, kind, api_type = record
    return f'    ("{name}", "{detail}", {kind}, "{api_type}"),'


def generate(model: dict, user_version, tracker_version, docs_version) -> str:
    classes = model["classes"]
    enums = model["enums"]
    datatypes = model["datatypes"]

    def member_sort_key(item):
        # item = (name, detail, kind, api_type)
        return (item[0].lower(), item[2], item[0])

    out = [HEADER]
    # Sort by raw name so `find_class`'s byte-wise binary_search_by matches.
    for name in sorted(classes, key=lambda n: n):
        cls = classes[name]
        members = sorted(cls["members"].values(), key=member_sort_key)
        # Cap per-class members at 256 to keep the binary lean; inherited
        # members are walked at runtime anyway.
        members = members[:256]
        out.append("    ApiClass {\n")
        out.append(f'        name: "{name}",\n')
        out.append(f'        superclass: "{cls["superclass"]}",\n')
        out.append(f'        datatype: {str(cls["datatype"]).lower()},\n')
        out.append("        members: &[\n")
        for record in members:
            out.append(render_member(record) + "\n")
        out.append("        ],\n")
        out.append("    },\n")

    enum_lines = []
    for name in sorted(enums, key=lambda n: n.lower()):
        items = enums[name]
        item_str = ", ".join(f'"{n}"' for n, _ in items)
        enum_lines.append(f'    ("{name}", &[{item_str}]),')
    out.append(FOOTER_TEMPLATE.format(
        enums="\n".join(enum_lines),
        libraries=_render_libraries(),
        global_types=_render_global_types(datatypes),
        bare_globals=_render_bare_globals(datatypes),
    ))
    return "".join(out)


def _render_libraries() -> str:
    lines = []
    for name in sorted(LUAU_LIBRARIES):
        lines.append(f'    ("{name}", &[')
        for member, detail, kind in sorted(LUAU_LIBRARIES[name], key=lambda m: m[0].lower()):
            lines.append(f'        ("{member}", "{detail}", {kind}, ""),')
        lines.append("    ]),")
    return "\n".join(lines)


def _render_global_types(datatypes: dict) -> str:
    lines = []
    for name in sorted(GLOBAL_TYPES, key=lambda n: n.lower()):
        lines.append(f'    ("{name}", "{GLOBAL_TYPES[name]}"),')
    for name in sorted(datatypes, key=lambda n: n.lower()):
        lines.append(f'    ("{name}", "{name}"),')
    for name in sorted(LUAU_LIBRARIES, key=lambda n: n.lower()):
        lines.append(f'    ("{name}", "@lib:{name}"),')
    return "\n".join(lines)


def _render_bare_globals(datatypes: dict) -> str:
    lines = []
    seen = set()
    for label, detail, insert in BARE_GLOBALS:
        if label.lower() in seen:
            continue
        seen.add(label.lower())
        insert_text = insert or label
        lines.append(f'    ("{label}", "{detail}", "{insert_text}"),')
    for name in sorted(LUAU_LIBRARIES, key=lambda n: n.lower()):
        if name.lower() in seen:
            continue
        seen.add(name.lower())
        lines.append(f'    ("{name}", "Luau {name} library", "{name}"),')
    for name in sorted(datatypes, key=lambda n: n.lower()):
        if name.lower() in seen:
            continue
        seen.add(name.lower())
        lines.append(f'    ("{name}", "Roblox datatype", "{name}"),')
    return "\n".join(lines)


# --------------------------------------------------------------------------
def main() -> int:
    if "--fetch" in sys.argv:
        fetch_sources()
    if not USER_DUMP.exists():
        print(f"error: missing {USER_DUMP}", file=sys.stderr)
        return 1

    user = load_user_dump()
    tracker = load_tracker_dump()
    docs = load_api_docs()
    if not tracker["classes"] and not docs:
        print("warning: tracker dump and api-docs missing; only the user dump "
              "will be used (no properties/datatypes). Run with --fetch.",
              file=sys.stderr)

    model = build_model(user, tracker, docs)
    source = generate(
        model,
        user_version=user["version"],
        tracker_version=tracker.get("version"),
        docs_version=None,
    )
    OUTPUT.write_text(source, encoding="utf-8")

    classes = model["classes"]
    members = sum(len(cls["members"]) for cls in classes.values())
    enums = sum(len(items) for _, items in model["enums"].items())
    print(f"wrote {OUTPUT.relative_to(ROOT)} "
          f"({OUTPUT.stat().st_size / 1024:.0f} KiB, "
          f"{len(classes)} classes/datatypes, {members} members, "
          f"{len(model['enums'])} enums/{enums} items)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
