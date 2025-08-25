local spec = {
  name = "dummy",
  category = "progress",
  weight = 999,
  capabilities = {
    basic = true,
  },
  detect = function() return true end,
  create = function(opts)
    return {
      open = function() end,
      stage_define = function() end,
      stage_update = function() end,
      update = function() end,
      finish = function() end,
    }
  end,
}
return spec
