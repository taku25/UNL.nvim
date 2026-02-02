-- lua/UNL/cmd/cd.lua (RPC Project Switching)
local scanner = require("UNL.scanner")
local unl_picker = require("UNL.backend.picker")
local unl_config = require("UNL.config")
local log = require("UNL.logging").get("UNL")
local unl_events = require("UNL.event.events")
local unl_event_types = require("UNL.event.types")

local M = {}

local rpc = require("UNL.rpc")
local registry = require("UNL.registry")

function M.execute(opts)
  log.debug("Fetching registered projects...")
  
  -- Internal function to show picker
  local function show_picker(projects)
      if #projects == 0 then
          log.warn("No registered projects found. Run :UNL setup in a project first.")
          return
      end

      local picker_items = {}
      for _, p in ipairs(projects) do
          local root = p.root
          local display_name = vim.fn.fnamemodify(root, ":t")
          table.insert(picker_items, {
              label = string.format("%s (%s)", display_name, root),
              value = root,
          })
      end

      table.sort(picker_items, function(a, b) return a.label < b.label end)

      unl_picker.pick({
          kind = "unl_project_cd",
          title = "Select Project to CD",
          items = picker_items,
          conf = unl_config.get("UNL"),
          preview_enabled = false,
          on_submit = function(selected_root)
              if not selected_root then return end
              local success, err = pcall(vim.api.nvim_set_current_dir, selected_root)
              unl_events.publish(unl_event_types.ON_AFTER_CHANGE_DIRECTORY, {
                  status = success and "success" or "failed",
                  new_cwd = selected_root,
                  error_message = err,
              })
              if success then
                  log.info("Changed directory to: %s", selected_root)
              else
                  log.error("Failed to cd to '%s': %s", selected_root, tostring(err))
              end
          end,
      })
  end

  rpc.request("list_projects", {}, nil, function(success, projects)
    if success and type(projects) == "table" then
        show_picker(projects)
    else
        log.warn("RPC list_projects failed, falling back to local registry.")
        local local_list = registry.load()
        show_picker(local_list)
    end
  end)
end

return M
