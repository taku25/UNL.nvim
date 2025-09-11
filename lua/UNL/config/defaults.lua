-- Centralized default configuration for UNL.
local M = {
  ui = {
    picker = {
      mode = "auto",
      prefer = { "telescope", "fzf-lua", "native", "dummy" },
    },
    grep_picker = {
      mode = "auto",
      prefer = { "telescope", "fzf-lua" }
    },
    find_picker = {
      mode = "auto",
      prefer = { "telescope", "fzf-lua" }
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
    debug_log = {
      position = "right",
      size = 0.4, -- 画面右側に40%の幅
    }, 
  },

  safe_open = {
    -- A list of buffer types to avoid.
    -- See `:help buftype` for more options.
    prevent_in_buftypes = {
      "nofile",
      "quickfix",
      "help",
      "terminal",
      "prompt",
    },
    -- A list of file types to avoid.
    prevent_in_filetypes = {
      "neo-tree",
      "NvimTree",
      "TelescopePrompt",
      "fugitive",
      "lazy",
    },
  },

  logging = {
    level = "info",
    echo = { level = "warn" },
    notify = { level = "error", prefix = "[UNL]" },
    file = { enable = true, max_kb = 512, rotate = 3, filename = "unl.log" },
    perf = { enabled = false, patterns = { "^refresh" }, level = "trace" },
    debug = { enable = true, },
  },

  cache = { dirname = "UNL_cache" },
  project = {
    localrc_filename = ".unlrc.json",
    search_stop_at_home = true,
    follow_symlink = true,
  },
  remote = {
    host = "127.0.0.1",
    port = 30010,
  },
}
return M
