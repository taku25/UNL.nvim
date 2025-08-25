local M = {}

local uv = vim.loop

local function tempdir()
  local base = (vim.fn.has("win32") == 1) and (os.getenv("TEMP") or "C:\\") or "/tmp"
  local template = base .. "/unl-config-test-XXXXXX"
  local dir = uv.fs_mkdtemp(template)
  assert(dir, "failed to create temp directory")
  return dir
end

local function write_json(path, tbl)
  local enc = (vim.json and vim.json.encode or vim.fn.json_encode)(tbl)
  vim.fn.writefile(vim.split(enc, "\n"), path)
end

local function rmdir_recursive(path)
  local h = uv.fs_scandir(path)
  if h then
    while true do
      local name, t = uv.fs_scandir_next(h)
      if not name then break end
      local child = path .. "/" .. name
      if t == "directory" then
        rmdir_recursive(child)
      else
        pcall(uv.fs_unlink, child)
      end
    end
  end
  pcall(uv.fs_rmdir, path)
end

local ctx_start_file_path = ""
function M.setup(opts)
  opts = opts or {}
  local Config = require("UNL.config")
  local config_default = require("UNL.config.defaults")
  Config.reset_single("UNL")

  local root = tempdir()
  local ctx = {
    root = root,
    user_cfg = opts.user or {},
    localrc = opts.localrc,
    rc_path = nil,
  }

  Config.setup("UNL", config_default,ctx.user_cfg)

  if ctx.localrc then
    local rc_name = Config.get("UNL").project.localrc_filename
    ctx.rc_path = root .. "/" .. rc_name
    write_json(ctx.rc_path, ctx.localrc)
  end

  ctx_start_file_path = opts.start_path_file or (root .. "/src/main.cpp")
  vim.fn.mkdir(vim.fs.dirname(ctx_start_file_path), "p")
  Config.reload("UNL",ctx_start_file_path)
  return ctx
end

function M.teardown(ctx)
  if ctx and ctx.root then
    rmdir_recursive(ctx.root)
  end
end

function M.cfg()
  return require("UNL.config").get("UNL", ctx_start_file_path)
end

return M
