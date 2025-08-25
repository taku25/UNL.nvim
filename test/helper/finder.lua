local uv = vim.loop

local M = {}

local function tempdir()
  local base
  if vim.fn.has("win32") == 1 then
    base = (os.getenv("TEMP") or "C:\\") .. "\\unl-finder-test-XXXXXX"
  else
    base = "/tmp/unl-finder-test-XXXXXX"
  end
  local dir = uv.fs_mkdtemp(base)
  assert(dir, "failed to create temp dir")
  return dir
end

local function rmdir_recursive(path)
  local handle = uv.fs_scandir(path)
  if handle then
    while true do
      local name, t = uv.fs_scandir_next(handle)
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

function M.new_ctx()
  local root = tempdir()
  local ctx = { root = root, paths = {} }
  return ctx
end

function M.add_dir(ctx, rel)
  local full = ctx.root .. "/" .. rel
  vim.fn.mkdir(full, "p")
  ctx.paths[#ctx.paths+1] = full
  return full
end

function M.add_file(ctx, rel, lines)
  local full = ctx.root .. "/" .. rel
  vim.fn.mkdir(vim.fs.dirname(full), "p")
  vim.fn.writefile(lines or { "// dummy" }, full)
  ctx.paths[#ctx.paths+1] = full
  return full
end

-- Build.cs 用ショートカット
function M.add_build_cs(ctx, rel, module_name)
  module_name = module_name or "MyModule"
  local content = {
    "// " .. module_name .. ".Build.cs",
    "public class " .. module_name .. " : ModuleRules { }",
  }
  return M.add_file(ctx, rel, content)
end

-- uproject (最小 JSON)
function M.add_uproject(ctx, rel, engine_association)
  local data = {
    FileVersion = 3,
    EngineAssociation = engine_association or "",
  }
  local enc = (vim.json and vim.json.encode or vim.fn.json_encode)(data)
  return M.add_file(ctx, rel, vim.split(enc, "\n"))
end

-- Engine 構造 (Engine/ ディレクトリだけ) を生成
function M.add_engine_root(ctx, rel)
  local root = M.add_dir(ctx, rel)
  M.add_dir(ctx, rel .. "/Engine")
  return root
end

function M.teardown(ctx)
  if ctx and ctx.root then
    rmdir_recursive(ctx.root)
  end
end

return M
