local H = {}

-- Collecting writer (captures all writes)
function H.collecting_writer(store)
  store = store or {}
  local self = {}
  function self.write(level, msg, ctx)
    store[#store+1] = {
      level = level,
      msg = msg,
      ctx = ctx,
    }
  end
  return self, store
end

-- Fake stdpath cache override (restore function returns original)
function H.override_stdpath(tmpdir)
  local orig = vim.fn.stdpath
  vim.fn.stdpath = function(which)
    if which == "cache" then
      return tmpdir
    end
    return orig(which)
  end
  return function()
    vim.fn.stdpath = orig
  end
end

-- Make temp directory (using luv)
function H.make_tempdir()
  local base = (vim.loop.os_tmpdir() or ".")
  local template = base .. "/unl_logger_test_XXXXXX"
  local path = vim.loop.fs_mkdtemp(template)
  assert(path, "failed to create temp dir")
  return path
end

-- Strip possible ANSI (not currently used but handy)
function H.strip_ansi(s)
  return s:gsub("\27%[[0-9;]*m", "")
end

return H
