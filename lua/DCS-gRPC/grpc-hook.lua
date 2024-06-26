-- note: the hook's load will only fire after the mission loaded.
local function load()
  log.write("[GRPC-Hook]", log.INFO, "mission loaded, setting up gRPC listener ...")

  -- Let DCS know where to find the DLLs
  if not string.find(package.cpath, GRPC.dllPath) then
    package.cpath = package.cpath .. [[;]] .. GRPC.dllPath .. [[?.dll;]]
  end

  local ok, grpc = pcall(require, "dcs_grpc_hot_reload")
  if ok then
    log.write("[GRPC-Hook]", log.INFO, "loaded hot reload version")
  else
    grpc = require("dcs_grpc")
  end

  _G.grpc = grpc
  assert(pcall(assert(loadfile(GRPC.luaPath .. [[grpc.lua]]))))

  log.write("[GRPC-Hook]", log.INFO, "gRPC listener set up.")
end

local handler = {}

function handler.onMissionLoadEnd()
  local ok, err = pcall(load)
  if not ok then
    log.write("[GRPC-Hook]", log.ERROR, "Failed to set up gRPC listener: "..tostring(err))
  end
end

function handler.onSimulationFrame()
  if GRPC.onSimulationFrame then
    GRPC.onSimulationFrame()
  end
end

function handler.onSimulationStop()
  log.write("[GRPC-Hook]", log.INFO, "simulation stopped, shutting down gRPC listener ...")

  GRPC.stop()
  grpc = nil
end

function handler.onPlayerTrySendChat(playerID, msg, side)

  -- we default to coalition.ALL chat (side = -1)
  local coalition = 0

  -- side -2 = Side Chat
  -- side -1 = All Chat
  -- side +2 = Server Messages to player

  if side == -2 then
    local playerInfo = net.get_player_info(playerID)
    if playerInfo ~= nil then
      coalition = playerInfo.side + 1
    else
      -- If we don't have a side, we're netural
      coalition = 1
    end
  end

  grpc.event({
    time = DCS.getModelTime(),
    event = {
      type = "playerSendChat",
      playerId = playerID,
      message = msg,
      coalition = coalition
    },
  })

  -- We do not return here, to allow other handlers to decide
end

function handler.onPlayerTryConnect(addr, name, ucid, id)
  grpc.event({
    time = DCS.getModelTime(),
    event = {
      type = "connect",
      addr = addr,
      name = name,
      ucid = ucid,
      id = id,
    },
  })
  -- not returning `true` here to allow other scripts to handle this hook
end

function handler.onPlayerDisconnect(id, reason)
  grpc.event({
    time = DCS.getModelTime(),
    event = {
      type = "disconnect",
      id = id,
      reason = reason + 1, -- Increment for non zero-indexed gRPC enum
    },
  })
end

function handler.onPlayerChangeSlot(playerId)
  local playerInfo = net.get_player_info(playerId)

  -- Default to 1 (spectators + 1 for grpc COALITION enum)
  local coalition = 1

  -- Default to empty, which matches spectators, which happens on relinquished
  -- disconnects, and other events where playerInfo goes to null
  local slot, groupName, unitType = ""

  if playerInfo ~= nil then
    coalition = playerInfo.side + 1 -- offsetting for grpc COALITION enum
    slot = playerInfo.slot
    groupName = DCS.getUnitProperty(playerInfo.slot, DCS.UNIT_GROUPNAME)
    unitType = DCS.getUnitProperty(playerInfo.slot, DCS.UNIT_TYPE)
  end

  grpc.event({
    time = DCS.getModelTime(),
    event = {
      type = "playerChangeSlot",
      playerId = playerId,
      coalition = coalition,
      slotId = slot,
      groupName = groupName,
      unitType = unitType,
    },
  })
end

DCS.setUserCallbacks(handler)
