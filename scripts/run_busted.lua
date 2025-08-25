-- script/run_busted.lua (auto-select output handler)
if _G.__UNL_BUSTED_ALREADY_RAN then
  return
end
_G.__UNL_BUSTED_ALREADY_RAN = true

local uv = vim.loop
local root = uv.cwd()
local test_dir = root .. "/test"

local function collect_specs(dir, acc)
  local h = uv.fs_scandir(dir)
  if not h then return end
  while true do
    local name, t = uv.fs_scandir_next(h)
    if not name then break end
    local full = dir .. "/" .. name
    if t == "directory" then
      collect_specs(full, acc)
    elseif name:match("_spec%.lua$") then
      acc[#acc+1] = full
    end
  end
end

local specs = {}
collect_specs(test_dir, specs)
table.sort(specs)

if #specs == 0 then
  io.stderr:write("[busted-runner] No *_spec.lua files under " .. test_dir .. "\n")
  vim.cmd("cquit")
  return
end

print(string.format("[busted-runner] Collected %d spec files", #specs))

local ok_factory, factory = pcall(require, "busted.runner")
if not ok_factory then
  io.stderr:write("[busted-runner] Cannot require 'busted.runner': " .. tostring(factory) .. "\n")
  vim.cmd("cquit")
  return
end

-- Determine available output handlers
-- local output_handler = "plain"
-- local ok_oph, oph = pcall(require, "busted.outputHandlers")
-- if ok_oph and type(oph) == "table" then
--   local preferred = { "utfTerminal", "plain", "tap", "TAP", "gtest", "junit", "json" }
--   local exists = {}
--   for name in pairs(oph) do
--     exists[name:lower()] = name
--   end
--   for _, p in ipairs(preferred) do
--     local got = exists[p:lower()]
--     if got then
--       output_handler = got
--       break
--     end
--   end
-- end
--
-- print("[busted-runner] Using output handler: " .. output_handler)

local args = {
  "--pattern=_spec",
}
for _, f in ipairs(specs) do
  args[#args+1] = f
end
_G.arg = args

local ok_run, runner_or_err = pcall(factory, { standalone = false })
if not ok_run then
  io.stderr:write("[busted-runner] Failed to init runner: " .. tostring(runner_or_err) .. "\n")
  vim.cmd("cquit")
  return
end

local exit_code
if type(runner_or_err) == "function" then
  local ok_exec, ret = pcall(runner_or_err)
  if not ok_exec then
    io.stderr:write("[busted-runner] Runtime error:\n" .. tostring(ret) .. "\n")
    vim.cmd("cquit")
    return
  end
  if type(ret) == "function" then
    local ok_exec2, ret2 = pcall(ret)
    if not ok_exec2 then
      io.stderr:write("[busted-runner] Second-stage run error:\n" .. tostring(ret2) .. "\n")
      vim.cmd("cquit")
      return
    end
    exit_code = ret2
  else
    exit_code = ret
  end
else
  exit_code = runner_or_err
end

if type(exit_code) ~= "number" then
  io.stderr:write("[busted-runner] Unexpected exit code type: " .. type(exit_code) .. "\n")
  vim.cmd("cquit")
  return
end

if exit_code == 0 then
  print("[busted-runner] OK (exit_code=0)")
  vim.cmd("quit")
else
  print("[busted-runner] FAIL (exit_code=" .. exit_code .. ")")
  vim.cmd("cquit")
end
