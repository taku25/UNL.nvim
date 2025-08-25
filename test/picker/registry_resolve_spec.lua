-- test/picker/registry_resolve_spec.lua

local helper = require("helper.picker_registry")

describe("UNL.backend.picker.registry.resolve", function()
  local ctx
  local registry

  -- このテストスイートで使う偽物のプロバイダ
  local fake_providers = {
    { name = "telescope", available = function() return true end },
    { name = "fzf_lua",   available = function() return true end },
    { name = "native",    available = function() return true end },
    { name = "dummy",     available = function() return true end },
  }

  before_each(function()
    ctx = helper.setup(fake_providers)
    registry = ctx.registry
  end)

  after_each(function()
    helper.teardown(ctx)
  end)

  it("should select the first available provider in 'auto' mode with default prefer", function()
    local conf = { mode = "auto" }
    local _, name = registry.resolve(conf, {})
    assert.are.equal("telescope", name)
  end)

  it("should respect the 'prefer' list in 'auto' mode", function()
    local conf = {
      mode = "auto",
      prefer = { "native", "telescope" }, -- nativeを優先
    }
    local _, name = registry.resolve(conf, {})
    assert.are.equal("native", name)
  end)
  
  it("should fall back to the next provider if the preferred one is unavailable", function()
    -- telescopeを利用不可にする
    fake_providers[1].available = function() return false end
    ctx = helper.setup(fake_providers) -- 再セットアップ
    
    local conf = {
      mode = "auto",
      prefer = { "telescope", "fzf_lua" },
    }
    local _, name = ctx.registry.resolve(conf, {})
    assert.are.equal("fzf_lua", name)
  end)

  it("should select a specific provider when mode is set directly", function()
    local conf = { mode = "native" }
    local _, name = registry.resolve(conf, {})
    assert.are.equal("native", name)
  end)

  it("should ignore unavailable provider when mode is set directly and fall back", function()
    -- fzf_luaを利用不可にする
    fake_providers[2].available = function() return false end
    ctx = helper.setup(fake_providers) -- 再セットアップ
    
    local conf = { mode = "fzf_lua" } -- fzf_luaを直接指定
    local _, name = ctx.registry.resolve(conf, {})
    
    -- fzf_luaが使えないので、最終フォールバックのdummyが選ばれる
    assert.are.equal("dummy", name)
  end)

  it("should return dummy provider if all preferred providers are unavailable", function()
    -- telescope と fzf_lua を利用不可にする
    fake_providers[1].available = function() return false end
    fake_providers[2].available = function() return false end
    ctx = helper.setup(fake_providers) -- 再セットアップ

    local conf = {
      mode = "auto",
      prefer = { "telescope", "fzf_lua" },
    }
    local _, name = ctx.registry.resolve(conf, {})

    -- preferリストのものは全て使えないので、最終フォールバックのdummyが選ばれる
    assert.are.equal("dummy", name)
  end)
end)
