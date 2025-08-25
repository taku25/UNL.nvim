local helper = require("helper.finder")
local engine  = require("UNL.finder.engine")
local Path    = require("UNL.path")

-- 期待: root が Engine/ を含むディレクトリ
-- engine.find_engine_root(uproject_path, opts) -> root|nil, err|nil

describe("finder.engine.find_engine_root - override path precedence", function()
  local ctx
  before_each(function()
    ctx = helper.new_ctx()
  end)
  after_each(function()
    helper.teardown(ctx)
  end)

  it("returns override when valid (even if uproject association empty)", function()
    local eroot = helper.add_engine_root(ctx, "UE/UE_5_6")
    helper.add_dir(ctx, "Game")
    local uproj = helper.add_uproject(ctx, "Game/MyGame.uproject", "")
    local root, err = engine.find_engine_root(uproj, {
      engine_override_path = eroot,
      debug = false,
      logger = { trace = function() end, warn = function() end },
    })
    assert.is_nil(err)
    assert.are.equal(Path.normalize(eroot), Path.normalize(root))
  end)

  it("fails when override path invalid (missing Engine/)", function()
    local invalid = helper.add_dir(ctx, "Broken/UEPath")
    helper.add_dir(ctx, "Game")
    local uproj = helper.add_uproject(ctx, "Game/MyGame.uproject", "")
    local root, err = engine.find_engine_root(uproj, {
      engine_override_path = invalid,
      logger = { trace = function() end, warn = function() end },
    })
    assert.is_nil(root)
    assert.is_truthy(err)
    assert.matches("override path invalid", err)
  end)
end)

describe("finder.engine.find_engine_root - EngineAssociation absolute path", function()
  local ctx
  before_each(function()
    ctx = helper.new_ctx()
  end)
  after_each(function()
    helper.teardown(ctx)
  end)

  it("accepts absolute association path that has Engine/", function()
    local eroot = helper.add_engine_root(ctx, "UE/Custom")
    helper.add_dir(ctx, "Game")
    local uproj = helper.add_uproject(ctx, "Game/MyGame.uproject", eroot) -- association = absolute path
    local root, err = engine.find_engine_root(uproj, {
      logger = { trace = function() end, warn = function() end },
    })
    assert.is_nil(err)
    assert.are.equal(Path.normalize(eroot), Path.normalize(root))
  end)

  it("fails for absolute association path missing Engine/", function()
    local bad = helper.add_dir(ctx, "UE/BadRoot")
    helper.add_dir(ctx, "Game")
    local uproj = helper.add_uproject(ctx, "Game/MyGame.uproject", bad)
    local root, err = engine.find_engine_root(uproj, {
      logger = { trace = function() end, warn = function() end },
    })
    assert.is_nil(root)
    assert.is_truthy(err)
    assert.matches("path invalid", err)
  end)
end)

describe("finder.engine.find_engine_root - embedded engine detection", function()
  local ctx
  before_each(function()
    ctx = helper.new_ctx()
  end)
  after_each(function()
    helper.teardown(ctx)
  end)

  it("finds engine root by walking up when EngineAssociation empty", function()
    local eroot = helper.add_engine_root(ctx, "UE/EmbeddedRoot")
    helper.add_dir(ctx, "UE/EmbeddedRoot/Projects/MyGame/Source")
    local uproj = helper.add_uproject(ctx, "UE/EmbeddedRoot/Projects/MyGame/MyGame.uproject", "")
    local root, err = engine.find_engine_root(uproj, {
      logger = { trace = function() end, warn = function() end },
    })
    assert.is_nil(err)
    assert.are.equal(Path.normalize(eroot), Path.normalize(root))
  end)
end)

describe("finder.engine.find_engine_root - failure when no association and not embedded", function()
  local ctx
  before_each(function()
    ctx = helper.new_ctx()
  end)
  after_each(function()
    helper.teardown(ctx)
  end)

  it("returns nil + error", function()
    helper.add_dir(ctx, "Game/Source")
    local uproj = helper.add_uproject(ctx, "Game/MyGame.uproject", "")
    local root, err = engine.find_engine_root(uproj, {
      logger = { trace = function() end, warn = function() end },
    })
    assert.is_nil(root)
    assert.is_truthy(err)
    -- 期待文言の一部 (実装: "no EngineAssociation and no embedded engine root")
    assert.matches("no EngineAssociation", err)
  end)
end)

-- オプション: GUID / Version (helper scripts) テスト
-- scripts/find_engine.(bat|sh) が存在する場合のみ実施
describe("finder.engine.find_engine_root - helper script (GUID / Version) integration", function()
  local ctx
  local have_script = false
  local script_path

  before_each(function()
    ctx = helper.new_ctx()

    -- プラグイン root 推定: engine.lua の find_plugin_root は自身のファイル位置から
    -- テスト環境で scripts ディレクトリを見つけられない場合は skip
    -- ここではリポジトリ直下 scripts/ を期待 (なければスキップ)
    local cwd = vim.loop.cwd()
    local bat = Path.join(cwd, "scripts", "find_engine.bat")
    local sh  = Path.join(cwd, "scripts", "find_engine.sh")
    if vim.fn.filereadable(bat) == 1 then
      have_script = true
      script_path = bat
    elseif vim.fn.filereadable(sh) == 1 then
      have_script = true
      script_path = sh
    end
  end)

  after_each(function()
    helper.teardown(ctx)
  end)

  it("GUID or version association resolves (conditionally)", function()
    if not have_script then
      pending("helper script not found; skipping GUID/version test")
      return
    end
    -- このテストはスクリプトが期待する出力に依存するため、
    -- ここではダミー構成に依存しない簡易検証に留めるか、
    -- スクリプトが受け取った引数を元に固定の Engine パスを返す実装であることを前提とする。
    -- 具体的な GUID / Version は適宜修正してください。
    helper.add_dir(ctx, "Game")
    local fake_guid = "{12345678-1234-1234-1234-1234567890ab}"
    local uproj = helper.add_uproject(ctx, "Game/MyGame.uproject", fake_guid)
    local root, err = engine.find_engine_root(uproj, {
      logger = { trace = function() end, warn = function() end },
      debug = true,
    })
    -- 成功か失敗かはスクリプト実装次第なので存在のみ検査しない:
    -- ここでは「root が返るなら Engine/ を持つ」 or 「返らないなら err がある」ことを保証
    if root then
      assert.is_true(vim.fn.isdirectory(Path.join(root, "Engine")) == 1)
    else
      assert.is_truthy(err)
    end
  end)
end)
