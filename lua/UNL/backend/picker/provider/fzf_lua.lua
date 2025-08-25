local M = { name = "fzf-lua" }

function M.available()
  return pcall(require, "fzf-lua")
end

function M.run(spec)
  local fzf = require("fzf-lua")
  local kind = spec.kind

  local entries = {}
  local entry_to_value = {}

  for _, entry in ipairs(spec.items) do
    local display = (spec.format and spec.format(entry)) or entry.label
    local value = entry.value
    local filename_for_preview = (type(value) == "table") and value.filename or (type(value) == "string" and value or nil)
    local line = display
    if filename_for_preview then
      line = line .. "\t" .. filename_for_preview
    end
    table.insert(entries, line)
    entry_to_value[line] = value
  end

  local enable_preview = true
  if spec.preview_enabled == false then
    enable_preview = false
  elseif spec.preview_enabled == true then
    enable_preview = true
  else
    if kind:match("project") and not kind:match("file") then
      enable_preview = false
    end
  end

  local opts = {
    prompt = spec.title or "Select Item",
    actions = {
      ["default"] = function(selected)
        local val = entry_to_value[selected[1]]
        if val and spec.on_submit then
          spec.on_submit(val)
        end
      end,
    },
  }

  if enable_preview then
    opts.previewer = function(item)
      local filename = item:match("\t(.+)$")
      if filename then
        return "cat " .. vim.fn.shellescape(filename)
      end
      return ""
    end
  else
    opts.previewer = false
  end

  -- ここを修正: fzf.fzf_exec で呼ぶ
  fzf.fzf_exec(entries, opts)
end

return M
