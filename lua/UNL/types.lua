---@meta
--- Type definitions for ancestor finder utility.
--- Provides upward (parent direction) directory search with flexible marker specification.
---
--- Public API (implementation in: UNL.finder.ancestor):
---   require("UNL.finder.ancestor").find_up_forward(start_path, markers, opts)
---
--- Markers argument accepted shapes:
---   1) string              : Single Lua pattern (filename:match(pattern))
---   2) string[]            : Array of Lua patterns (OR condition)
---   3) function            : Custom checker  (dir, original_markers, opts) -> string|nil
---                            original_markers is the function itself
---   4) nil                 : No search (immediately returns nil)
---
--- Return:
---   First matching directory path or nil if none found within max_depth.
---
--- Example patterns:
---   "%.uproject$"
---   "%.[Uu][Pp][Rr][Oo][Jj][Ee][Cc][Tt]$"
---
--- Example usages:
---   local ancestor = require("UNL.finder.ancestor")
---   local root = ancestor.find_up_forward(vim.fn.getcwd(), "%.uproject$")
---
---   local root2 = ancestor.find_up_forward(vim.fn.expand("%:p"), {
---     "%.[Uu][Pp][Rr][Oo][Jj][Ee][Cc][Tt]$",
---     "^UE%.ini$",
---   }, { debug = true })
---
---   local root3 = ancestor.find_up_forward(vim.fn.getcwd(), function(dir)
---     for name, t in vim.fs.dir(dir) do
---       if t == "file" and name:lower() == ".ueprc" then
---         return dir
---       end
---     end
---     return nil
---   end)
---
---   local noisy = ancestor.find_up_forward(vim.fn.getcwd(), "%.uproject$", {
---     logger = {
---       trace = function(_, msg) vim.notify(msg, vim.log.levels.TRACE) end,
---       warn  = function(_, msg) vim.notify(msg, vim.log.levels.WARN) end,
---     },
---     debug = true,
---     debug_files = true,
---     debug_files_limit = 10,
---     on_search_path = function(p) vim.schedule(function() print("SCAN:", p) end) end,
---   })

--------------------------------------------------
-- Logger
--------------------------------------------------

---@class UNL.Logger
---@field trace fun(self:UNL.Logger, msg:string) | nil
---@field warn  fun(self:UNL.Logger, msg:string) | nil

--------------------------------------------------
-- Options
--------------------------------------------------

---@class UNL.AncestorFindOptions
---@field max_depth? integer                     # Maximum upward steps (default 120)
---@field on_search_path? fun(path:string)       # Called each level (pcall-wrapped)
---@field logger? UNL.Logger                     # Dependency-injected logger (trace / warn)
---@field debug? boolean                         # Enable depth / match logs
---@field debug_files? boolean                   # Log a (capped) file list per directory
---@field debug_files_limit? integer             # Cap for debug file list (default 40)

--------------------------------------------------
-- Marker function type
--------------------------------------------------

---@alias UNL.AncestorMarkerFunction fun(dir:string, original_markers:any, opts:UNL.AncestorFindOptions): (string|nil)

--------------------------------------------------
-- Markers union
--------------------------------------------------

---@alias UNL.AncestorMarkers
---| string
---| string[]
---| UNL.AncestorMarkerFunction
---| nil

--------------------------------------------------
-- Module interface
--------------------------------------------------

---@class UNL.Finder.Ancestor
---@field find_up_forward fun(start_path:string, markers:UNL.AncestorMarkers, opts?:UNL.AncestorFindOptions): (string|nil)

--------------------------------------------------
-- Return a dummy table purely for type inference when this file is required directly.
-- (In normal use you would: require("UNL.finder.ancestor") which returns the real implementation.)
--------------------------------------------------
local _M = {} ---@type UNL.Finder.Ancestor
return _M
