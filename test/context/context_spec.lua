local context = require("UNL.context")

describe("UNL.context (runtime context store)", function()
  local ns = "testns"
  local project_root = "/tmp/testproj"
  local data = "foo"

  before_each(function()
    context.reset()
  end)

  it("can create a namespace and project, set/get/del keys", function()
    local ns_handle = context.use(ns)
    local proj = ns_handle:key(project_root)

    -- set
    proj:set(data, 123)
    assert.are.equal(123, proj:get(data))

    -- overwrite
    proj:set(data, 456)
    assert.are.equal(456, proj:get(data))

    -- delete
    proj:del(data)
    assert.is_nil(proj:get(data))
  end)

  it("can list all keys", function()
    local h = context.use(ns):key(project_root)
    h:set("a", 1)
    h:set("b", 2)
    local all = h:all()
    assert.are.same({ a = 1, b = 2 }, all)
  end)

  it("supports generation and bump_generation", function()
    local h = context.use(ns):key(project_root)
    assert.are.equal(0, h:generation())
    h:generation(10)
    assert.are.equal(10, h:generation())
    h:bump_generation()
    assert.are.equal(11, h:generation())
  end)

  -- it("supports merge_layer and materialize_config", function()
  --   local h = context.use(ns):key(project_root)
  --   h:merge_layer("base", { foo = 1 }, 1)
  --   h:merge_layer("override", { bar = 2, foo = 42 }, 2)
  --   local mat = h:materialize_config()
  --   assert.are.same({ foo = 42, bar = 2 }, mat)
  -- end)

  it("can subscribe and receive events", function()
    local received = {}
    local sid = context.subscribe(ns, "set", function(ev)
      table.insert(received, ev)
    end)
    local h = context.use(ns):key(project_root)
    h:set("k", "v")
    assert.are.equal(1, #received)
    assert.are.equal("k", received[1].payload.data_key)
    assert.are.equal("v", received[1].payload.new)
    assert.are.equal(project_root, received[1].payload.key)
    context.unsubscribe(sid)
  end)

  it("can list and clear projects in namespace", function()
    local n = context.use(ns)
    n:key("/tmp/p1"):set("x", 1)
    n:key("/tmp/p2"):set("y", 2)
    local list = context.list_keys(ns)
    assert.are.same({ "/tmp/p1", "/tmp/p2" }, list)
    context.clear_namespace(ns)
    assert.are.same({}, context.list_keys(ns))
  end)

  it("reset wipes all state", function()
    local h = context.use(ns):key(project_root)
    h:set("a", 1)
    context.reset()
    local h2 = context.use(ns):key(project_root)
    assert.is_nil(h2:get("a"))
  end)
end)
