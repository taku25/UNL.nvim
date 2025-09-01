-- Centralized default configuration for UNL.
local M = {
  ui = {
    picker = {
      mode = "auto",
      prefer = { "telescope", "fzf-lua", "native", "dummy" },
    },
    filer = {
      mode = "auto",
      prefer = { "nvim-tree", "neo-tree", "native", "dummy"  },
    },
    progress = {
      mode = "auto",
      enable = true,
      prefer = { "fidget", "generic_status", "window", "notify", "dummy" },
      allow_regression = false,
    },
  },
  logging = {
    level = "info",
    echo = { level = "warn" },
    notify = { level = "error", prefix = "[UNL]" },
    file = { enable = true, max_kb = 512, rotate = 3, filename = "unl.log" },
    perf = { enabled = false, patterns = { "^refresh" }, level = "trace" },
  },
  cache = { dirname = "UNL_cache" },
  project = {
    localrc_filename = ".unlrc.json",
    search_stop_at_home = true,
    follow_symlink = true,
  },
}
return M
