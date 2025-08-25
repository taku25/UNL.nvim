local helper = require("helper.finder")
local Path = require("UNL.path")



describe("finder.module.find_module_root", function()
  local ctx
  local finder

  before_each(function()
    ctx = helper.new_ctx()
    finder = require("UNL.finder.module")

    -- 階層: root/a/b/c/src/file.cpp
    -- モジュールの Build.cs は b ディレクトリ内: root/a/b/MyModule.Build.cs
    helper.add_dir(ctx, "a/b/c/src")
    helper.add_file(ctx, "a/b/c/src/file.cpp", { "// source" })
    helper.add_build_cs(ctx, "a/b/MyModule.Build.cs", "MyModule")
  end)

  after_each(function()
    helper.teardown(ctx)
  end)

  it("ascends to directory that owns the Build.cs file", function()
    local start = ctx.root .. "/a/b/c/src/file.cpp"
    local mod_root = finder.find_module_root(start, {
      debug = false,
      logger = {
        trace = function() end,
        warn = function(msg) vim.notify(msg, vim.log.levels.WARN) end,
      },
    })
    assert.is_truthy(mod_root)

    local expected = Path.join(ctx.root, "a", "b")
    local actual = Path.normalize(mod_root)
    assert.are.equal(expected, actual)
  end)

  it("returns nil if no Build.cs present upward", function()
    local start = ctx.root .. "/a/b/c/src/file.cpp"
    os.remove(ctx.root .. "/a/b/MyModule.Build.cs")
    local mod_root = finder.find_module_root(start, {})
    assert.is_nil(mod_root)
  end)
end)
