--
-- Custom Weapon Handling
--

GRPC.methods.getWeaponTransform = function(params)
  GRPC.logWarning("lua call getWeaponTransform(" .. tostring(params.id) .. ")")

  local weapon = GRPC.state.tracked_weapons[params.id]
  if weapon == nil then
    local msg = "weapon " .. tostring(params.id) .. " does not exist"
    GRPC.logWarning(msg)
    return GRPC.errorNotFound(msg)
  end

  if not weapon.isExist then
    local msg = "weapon object missing isExist " .. tostring(params.id) .. " does not exist"
    GRPC.logWarning(msg)
    return GRPC.errorNotFound(msg)
  end

  if not weapon:isExist() then
    local msg = "weapon " .. tostring(params.id) .. " does not exist"
    GRPC.logWarning(msg)
    return GRPC.errorNotFound(msg)
  end

  local retval = {
    time = timer.getTime(),
    rawTransform = GRPC.exporters.rawTransform(weapon)
  }

  GRPC.logWarning("lua response getWeaponTransform(" .. tostring(params.id) .. "): " .. net.lua2json(retval))
  return GRPC.success(retval)
end


GRPC.methods.weaponDestroy = function(params)
  local weapon = GRPC.state.tracked_weapons[params.id]
  if weapon == nil then
    return GRPC.errorNotFound("weapon " .. tostring(params.id) .. " does not exist")
  end

  if not weapon.isExist or not weapon:isExist() then
    GRPC.state.tracked_weapons[params.id] = nil
    return GRPC.errorNotFound("weapon " .. tostring(params.id) .. " does not exist")
  end

  -- We can remove it here too since we destroyed it
  weapon:Destroy()
  GRPC.state.tracked_weapons[params.id] = nil
  return GRPC.success({})
end


GRPC.methods.getTrackedWeaponIds = function()

  local weapon_ids = {}

  for k, _ in pairs(GRPC.state.tracked_weapons) do
    table.insert(weapon_ids, k)
  end

  return GRPC.success({
    weaponIds = weapon_ids
  })

end
