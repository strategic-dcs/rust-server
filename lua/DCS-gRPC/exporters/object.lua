--
-- Converts DCS tables in the Object hierarchy into tables suitable for
-- serialization into GRPC responses
-- Each exporter has an equivalent .proto Message defined and they must
-- be kept in sync
--

GRPC.exporters.position = function(pos)
  local lat, lon, alt = coord.LOtoLL(pos)
  return {
    lat = lat,
    lon = lon,
    alt = alt,
    u = pos.z,
    v = pos.x,
  }
end

GRPC.exporters.unit = function(unit)

  if unit == nil then return nil end

  -- Try and get the unit id as sometimes this errors with "Unit doesn't exist"
  -- we dont use isExist() because sometimes isExist returns false whilst all
  -- the data is still accessible and we want it!
  local status, unit_id = pcall(unit.getID, unit)
  if status == false then return nil end

  local group = unit:getGroup()
  if group then
    group = GRPC.exporters.group(group)
  end

  return {
    id = tonumber(unit_id),
    name = unit:getName(),
    callsign = unit:getCallsign(),
    coalition = unit:getCoalition() + 1, -- Increment for non zero-indexed gRPC enum
    type = unit:getTypeName(),
    playerName = Unit.getPlayerName(unit),
    group = group,
    numberInGroup = unit:getNumber(),
    inAir = unit:inAir(),
    fuel = unit:getFuel(),
    rawTransform = GRPC.exporters.rawTransform(unit),
  }
end

-- Data used to calculate position/orientation/velocity on the Rust side.
GRPC.exporters.rawTransform = function(object)
  local p = object:getPosition()
  local position = GRPC.exporters.position(p.p)
  return {
    position = position,
    positionNorth = coord.LLtoLO(position.lat + 1, position.lon),
    forward = p.x,
    right = p.z,
    up = p.y,
    velocity = object:getVelocity(),
  }
end

GRPC.exporters.group = function(group)
  return {
    id = tonumber(group:getID()),
    name = group:getName(),
    coalition = group:getCoalition() + 1, -- Increment for non zero-indexed gRPC enum
    category = group:getCategory() + 1, -- Increment for non zero-indexed gRPC enum
  }
end

GRPC.exporters.weapon = function(weapon)

  -- Skip weapons that don't exist otherwise getTypeName, getCategory, and
  -- rawTransform all fail as it doesn't exist
  if weapon == nil or not weapon.isExist or not weapon:isExist() then
    return nil
  end

  return {
    id = weapon:tonumber(),
    type = weapon:getTypeName(),
    category = weapon:getCategory(),
    rawTransform = GRPC.exporters.rawTransform(weapon),
  }
end

GRPC.exporters.static = function(static)
  return {
    id = tonumber(static:getID()),
    type = static:getTypeName(),
    name = static:getName(),
    coalition = static:getCoalition() + 1, -- Increment for non zero-indexed gRPC enum
    rawTransform = GRPC.exporters.rawTransform(static),
  }
end

GRPC.exporters.airbase = function(airbase)

  -- Airbase may not exist (deleted farp, moved farp)
  if airbase == nil or not airbase.isExist or not airbase:isExist() then return nil end

  local a = {
    name = airbase:getName(),
    callsign = airbase:getCallsign(),
    coalition = airbase:getCoalition() + 1, -- Increment for non zero-indexed gRPC enum
    category = airbase:getDesc()['category'] + 1, -- Increment for non zero-indexed gRPC enum
    displayName = airbase:getDesc()['displayName'],
    position = GRPC.exporters.position(airbase:getPoint())
  }

  local unit = airbase:getUnit()
  if unit then
    a.unit = GRPC.exporters.unit(unit)
  end

  return a
end

GRPC.exporters.scenery = function(scenery)
  return {
    id = tonumber(scenery:getName()),
    type = scenery:getTypeName(),
    position = GRPC.exporters.position(scenery:getPoint()),
  }
end

GRPC.exporters.cargo = function()
  return {}
end

-- every object, even an unknown one, should at least have getName implemented as it is
-- in the base object of the hierarchy
-- https://wiki.hoggitworld.com/view/DCS_Class_Object
GRPC.exporters.unknown = function(object)
  return {
    name = tostring(object:getName()),
  }
end

GRPC.exporters.markPanel = function(markPanel)
  local mp = {
    id = markPanel.idx,
    time = markPanel.time,
    text = markPanel.text,
    position = GRPC.exporters.position(markPanel.pos),
  }

  if markPanel.initiator then
    mp.initiator = GRPC.exporters.unit(markPanel.initiator)
  end

  if (markPanel.coalition >= 0 and markPanel.coalition <= 2) then
    mp.coalition = markPanel.coalition + 1; -- Increment for non zero-indexed gRPC enum
  end

  if (markPanel.groupID > 0) then
    mp.groupId = markPanel.groupID;
  end

  return mp
end
