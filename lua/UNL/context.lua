-- UNL.context
-- 汎用ランタイムストア: namespace -> key -> state
-- UEP / UBT / UCM などが共通で現在の設定や状態を共有するための軽量レジストリ

local uv = vim.loop

local M = {
  _ns = {},          -- ns -> key -> state
  _subs = {},        -- id -> { ns, event, cb }
  _next_sub_id = 1,
}

local function now_ms()
  return uv.hrtime() / 1e6
end

local function ensure_key_store(ns_tbl, key)
  local store = ns_tbl[key]
  if not store then
    store = {
      keys = {},          -- data_key -> { value, updated_at, meta }
      generation = 0,
    }
    ns_tbl[key] = store
  end
  return store
end

local function emit(self, ns, event, payload)
  local log = require("UNL.logging").get("UNL")
  for id, sub in pairs(self._subs) do
    if sub.ns == ns and (sub.event == event or sub.event == "any") then
      local ok, err = pcall(sub.cb, {
        ns = ns,
        event = event,
        payload = payload,
        id = id,
      })
      if not ok then
        vim.schedule(function()
          log.error(("UNL.context subscriber error: %s"):format(err))
        end)
      end
    end
  end
end

-- Subscription API
function M.subscribe(ns, event, cb)
  local id = M._next_sub_id
  M._next_sub_id = id + 1
  M._subs[id] = { ns = ns, event = event, cb = cb }
  return id
end

function M.unsubscribe(id)
  M._subs[id] = nil
end

-- Namespace handle
local NsHandle = {}
NsHandle.__index = NsHandle

function M.use(ns)
  if not M._ns[ns] then
    M._ns[ns] = {}
  end
  return setmetatable({ _nsname = ns, _ns_tbl = M._ns[ns] }, NsHandle)
end

-- Key-specific handle (以前の ProjectHandle)
local KeyHandle = {}
KeyHandle.__index = KeyHandle

function NsHandle:key(key)
  local store = ensure_key_store(self._ns_tbl, key)
  return setmetatable({
    _nsname = self._nsname,
    _ns_tbl = self._ns_tbl,
    _key = key,
    _store = store,
  }, KeyHandle)
end

-- Key-value CRUD
function KeyHandle:set(data_key, value, meta)
  local old = self._store.keys[data_key]
  self._store.keys[data_key] = {
    value = value,
    updated_at = now_ms(),
    meta = meta,
  }
  emit(M, self._nsname, "set", {
    key = self._key,
    data_key = data_key,
    old = old and old.value or nil,
    new = value,
  })
end

function KeyHandle:get(data_key)
  local ent = self._store.keys[data_key]
  return ent and ent.value or nil
end

function KeyHandle:del(data_key)
  local old = self._store.keys[data_key]
  if old then
    self._store.keys[data_key] = nil
    emit(M, self._nsname, "delete", {
      key = self._key,
      data_key = data_key,
      old = old.value,
    })
  end
end

function KeyHandle:all()
  local out = {}
  for k,v in pairs(self._store.keys) do
    out[k] = v.value
  end
  return out
end

-- Generation
function KeyHandle:generation(new_gen)
  if new_gen ~= nil then
    local old = self._store.generation
    self._store.generation = new_gen
    emit(M, self._nsname, "generation", {
      key = self._key,
      old = old,
      new = new_gen,
    })
  end
  return self._store.generation
end

function KeyHandle:bump_generation()
  return self:generation(self._store.generation + 1)
end

-- Listing / cleanup
function M.list_keys(ns)
  local ret = {}
  local tbl = M._ns[ns]
  if not tbl then return ret end
  for k,_ in pairs(tbl) do
    table.insert(ret, k)
  end
  table.sort(ret)
  return ret
end

function M.clear_namespace(ns)
  M._ns[ns] = {}
end

function M.reset()
  M._ns = {}
  M._subs = {}
  M._next_sub_id = 1
end

return M
