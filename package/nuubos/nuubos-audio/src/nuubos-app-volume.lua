-- nuubOS: a new application playback stream starts at the Applications
-- (Quick Menu "Game") volume, node.stream.default-playback-volume, which
-- nuubos-pw-app-volume sets. WirePlumber's own state-stream applies that
-- default only with node.stream.restore-props on, which nuubOS keeps off
-- (one global gain, no per-application memory): without this hook a game
-- started at 100 % until the slider moved.
-- Product Audio and the Media music stay at unity, as in nuubos-pw-app-volume.

log = Log.open_topic ("s-nuubos-app-volume")

local function is_product_stream (props)
  local node_name = props ["node.name"] or ""
  return node_name:find ("^alsa_playback%.nuubos%-audio%-") ~= nil or
      props ["application.name"] == "nuubOS Music"
end

local function channel_count (node)
  for pod in node:iterate_params ("Format") do
    local parsed = pod:parse ()
    if parsed ~= nil then
      local channels = parsed.properties.channels
      if type (channels) == "number" then
        return channels
      elseif type (channels) == "table" and #channels > 0 then
        return channels [1]
      end
    end
  end
  return 2
end

SimpleEventHook {
  name = "node/nuubos-app-volume",
  interests = {
    EventInterest {
      Constraint { "event.type", "=", "node-added" },
      Constraint { "media.class", "=", "Stream/Output/Audio" },
    },
  },
  execute = function (event)
    local node = event:get_subject ()

    if Settings.get_boolean ("node.stream.restore-props") or
        is_product_stream (node.properties) then
      return
    end

    -- The setting holds the slider fraction; wpctl set-volume (the helper,
    -- for running streams) maps it cubically.
    local fraction = Settings.get_float ("node.stream.default-playback-volume")
    local volume = fraction * fraction * fraction
    local volumes = { "Spa:Float" }
    for _ = 1, channel_count (node) do
      table.insert (volumes, volume)
    end
    log:info (node, "Applications volume " .. tostring (fraction))
    node:set_param ("Props", Pod.Object {
      "Spa:Pod:Object:Param:Props", "Props",
      channelVolumes = Pod.Array (volumes),
    })
  end
}:register ()
