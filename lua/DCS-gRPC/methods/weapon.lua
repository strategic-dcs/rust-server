--
-- Custom Weapon Handling
--

GRPC.methods.getWeaponTransform = function(params)
  local weapon = GRPC.state.tracked_weapons[params.id]
  if weapon == nil then
    GRPC.state.tracked_weapons[params.id] = nil
    local msg = "weapon " .. tostring(params.id) .. " does not exist"
    return GRPC.errorNotFound(msg)
  end

  if not weapon.isExist then
    GRPC.state.tracked_weapons[params.id] = nil
    local msg = "weapon object missing isExist " .. tostring(params.id) .. " does not exist"
    return GRPC.errorNotFound(msg)
  end

  if not weapon:isExist() then
    GRPC.state.tracked_weapons[params.id] = nil
    local msg = "weapon " .. tostring(params.id) .. " does not exist"
    return GRPC.errorNotFound(msg)
  end

  return GRPC.success({
    time = timer.getTime(),
    rawTransform = GRPC.exporters.rawTransform(weapon)
  })
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
