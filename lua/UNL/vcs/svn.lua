-- lua/UNL/vcs/svn.lua
local M = {}

function M.get_hash(root)
    local output = vim.fn.systemlist("svn info --show-item revision " .. vim.fn.shellescape(root))
    if vim.v.shell_error == 0 and #output > 0 then
        return "svn:" .. output[1]
    end
    return nil
end

return M
