//! Test fixtures compiled at test time by real Lua 5.3 (mlua, vendored) from our own source, so
//! no bytecode is ever committed (ground rule 3; the forbidden-file check rejects it anyway).
#![allow(dead_code)]

/// A module that touches every constant type, closures and upvalues, numeric and generic `for`,
/// varargs, methods, integer division, `goto`, and a global call guarded by `IsServer()`.
pub const MODULE: &str = r#"local M = {}
local server = { host = "127.0.0.1", port = 7804, ratio = 0.5, on = true, off = false, none = nil }
function M.pick(list, name)
  for i, s in ipairs(list) do
    if s.name == name then return s, i end
  end
  return nil
end
function M:connect(opts)
  local h = opts and opts.host or server.host
  if IsServer() then return string.format("%s:%d", h, server.port) end
  return h
end
local n = 0
for i = 1, 10, 2 do n = n + i // 2 end
local function count(...) return select('#', ...) end
M.count = count
goto done
::done::
return M
"#;

/// Method calls where jumps land: at the start of a `for` body (the FORLOOP target) and of an
/// `else` branch, plus a chained call and one in a `while` body.
pub const METHODS: &str = r#"local obj = { n = 0 }
function obj:add(k) self.n = self.n + k return self end
for i = 1, 3 do obj:add(i) end
if obj.n > 5 then obj:add(1) else obj:add(2):add(3) end
while obj.n < 20 do obj:add(4) end
return obj:add(0).n
"#;

/// Our test sources: the module, the method calls, a one-liner, a long string (over 254 bytes, so
/// its length is written as 0xFF plus a size_t), and a generic loop over `_G`.
pub fn sources() -> Vec<String> {
    vec![
        MODULE.to_string(),
        METHODS.to_string(),
        "print('hello')".to_string(),
        format!("local s = \"{}\" return s, #s", "x".repeat(300)),
        "local t = {} for k, v in pairs(_G) do t[k] = type(v) end return t".to_string(),
    ]
}

/// `src` compiled by Lua 5.3 and dumped: stripped (as all of the client's chunks are) or not.
pub fn luac(src: &str, strip: bool) -> Vec<u8> {
    let lua = mlua::Lua::new();
    lua.load(src)
        .into_function()
        .expect("our test source compiles")
        .dump(strip)
}

/// Every source, both stripped and with debug info.
pub fn fixtures() -> Vec<Vec<u8>> {
    sources()
        .iter()
        .flat_map(|s| [luac(s, true), luac(s, false)])
        .collect()
}
