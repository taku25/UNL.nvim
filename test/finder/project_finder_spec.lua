local helper = require("helper.finder")
local project = require("UNL.finder.project")
local Path = require("UNL.path")

local function upath(...)
  return Path.join(...)
end

describe("finder.project.find_project (basic strategies)", function()
  local ctx
  before_each(function()
    ctx = helper.new_ctx()
    -- 構造:
    --   <tmp>/ProjRoot/Foo.uproject
    --   <tmp>/ProjRoot/BarLongName.uproject
    --   <tmp>/ProjRoot/Zeta.uproject
    --   <tmp>/ProjRoot/deep/nest/file.cpp (探索開始)
    helper.add_dir(ctx, "ProjRoot")
    -- 生成順をバラして列挙順依存性を下げる
    helper.add_uproject(ctx, "ProjRoot/BarLongName.uproject")
    helper.add_uproject(ctx, "ProjRoot/Foo.uproject")
    helper.add_uproject(ctx, "ProjRoot/Zeta.uproject")

    helper.add_dir(ctx, "ProjRoot/deep/nest")
    helper.add_file(ctx, "ProjRoot/deep/nest/file.cpp")
  end)

  after_each(function()
    helper.teardown(ctx)
  end)

  local function start_path()
    return upath(ctx.root, "ProjRoot", "deep", "nest", "file.cpp")
  end

  it("strategy=first returns some project (not asserting name because FS enumeration order may vary)", function()
    local res = project.find_project(start_path(), { select_strategy = "first" })
    assert.is_truthy(res)
    assert.is_true(Path.equal(res.root, upath(ctx.root, "ProjRoot")))
    assert.is_true(res.uproject:match("%.uproject$") ~= nil)
  end)

  it("strategy=shortest picks Foo.uproject", function()
    local res = project.find_project(start_path(), { select_strategy = "shortest" })
    assert.is_truthy(res)
    assert.are.equal(upath(ctx.root, "ProjRoot", "Foo.uproject"), Path.normalize(res.uproject))
  end)

  it("strategy=longest picks BarLongName.uproject", function()
    local res = project.find_project(start_path(), { select_strategy = "longest" })
    assert.is_truthy(res)
    assert.are.equal(upath(ctx.root, "ProjRoot", "BarLongName.uproject"), Path.normalize(res.uproject))
  end)

  it("strategy=alphabetical picks BarLongName.uproject (B < F < Z)", function()
    local res = project.find_project(start_path(), { select_strategy = "alphabetical" })
    assert.is_truthy(res)
    assert.are.equal(upath(ctx.root, "ProjRoot", "BarLongName.uproject"), Path.normalize(res.uproject))
  end)
end)

describe("finder.project.find_project (accept_pattern / filter / depth)", function()
  local ctx
  before_each(function()
    ctx = helper.new_ctx()
    helper.add_dir(ctx, "GameRoot")
    helper.add_uproject(ctx, "GameRoot/GameEditor.uproject")
    helper.add_uproject(ctx, "GameRoot/GameClient.uproject")
    helper.add_uproject(ctx, "GameRoot/Tests.testproject") -- ノイズ (accept_pattern で除外)
    helper.add_dir(ctx, "GameRoot/Sub/Deeper/More")
    helper.add_file(ctx, "GameRoot/Sub/Deeper/More/source.cpp")
  end)

  after_each(function()
    helper.teardown(ctx)
  end)

  local function deep_start()
    return upath(ctx.root, "GameRoot", "Sub", "Deeper", "More", "source.cpp")
  end

  it("accept_pattern narrows candidates (only *Client.uproject)", function()
    local res = project.find_project(deep_start(), {
      accept_pattern = "Client%.uproject$",
      select_strategy = "alphabetical",
    })
    assert.is_truthy(res)
    assert.are.equal(upath(ctx.root, "GameRoot", "GameClient.uproject"), Path.normalize(res.uproject))
  end)

  it("filter excludes a specific file (removes *Client -> *Editor selected)", function()
    local res = project.find_project(deep_start(), {
      filter = function(fname)
        return not fname:match("Client%.uproject$")
      end,
      select_strategy = "alphabetical",
    })
    assert.is_truthy(res)
    assert.are.equal(upath(ctx.root, "GameRoot", "GameEditor.uproject"), Path.normalize(res.uproject))
  end)

  it("max_depth too small returns nil", function()
    -- 深さ: GameRoot/Sub/Deeper/More から GameRoot まで 3
    local res = project.find_project(deep_start(), {
      max_depth = 1,
      select_strategy = "shortest",
    })
    assert.is_nil(res)
  end)

  it("start_path is a directory (not a file) still works", function()
    local start_dir = upath(ctx.root, "GameRoot", "Sub", "Deeper", "More")
    local res = project.find_project(start_dir, { select_strategy = "shortest" })
    assert.is_truthy(res)
    -- shortest: GameClient (base 10 chars) vs GameEditor (9?) → 実際は "GameClient" (10) と "GameEditor" (10) 同長なので alphabetical 次第
    -- 同長ケース: pick_candidate の shortest では同長なら lexicographical 比較 → GameClient < GameEditor (C < E)
    assert.are.equal(upath(ctx.root, "GameRoot", "GameClient.uproject"), Path.normalize(res.uproject))
  end)
end)

describe("finder.project.find_project_root / find_project_file wrappers", function()
  local ctx
  before_each(function()
    ctx = helper.new_ctx()
    helper.add_dir(ctx, "P")
    helper.add_uproject(ctx, "P/A.uproject")
    helper.add_uproject(ctx, "P/BBBB.uproject")
    helper.add_dir(ctx, "P/deep")
    helper.add_file(ctx, "P/deep/x.cpp")
  end)

  after_each(function()
    helper.teardown(ctx)
  end)

  it("find_project_root returns directory only", function()
    local root = project.find_project_root(upath(ctx.root, "P", "deep", "x.cpp"), { select_strategy = "alphabetical" })
    assert.is_truthy(root)
    assert.is_true(Path.equal(root, upath(ctx.root, "P")))
  end)

  it("find_project_file returns chosen uproject path", function()
    local file = project.find_project_file(upath(ctx.root, "P", "deep", "x.cpp"), { select_strategy = "alphabetical" })
    assert.is_truthy(file)
    assert.are.equal(upath(ctx.root, "P", "A.uproject"), Path.normalize(file))
  end)

  it("returns nil when no uproject upward", function()
    local nores = project.find_project_root(upath(ctx.root, "P", "deep", "x.cpp"), {
      accept_pattern = "%.unlikely$",
    })
    assert.is_nil(nores)
  end)
end)
