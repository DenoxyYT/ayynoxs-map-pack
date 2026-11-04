package de.ayynoxs.mapsync;

import java.io.ByteArrayOutputStream;
import java.nio.charset.StandardCharsets;
import java.util.HashMap;
import java.util.Map;
import java.util.UUID;
import org.bukkit.entity.Player;
import org.bukkit.event.EventHandler;
import org.bukkit.event.Listener;
import org.bukkit.event.player.PlayerQuitEvent;
import org.bukkit.plugin.java.JavaPlugin;
import org.bukkit.plugin.messaging.PluginMessageListener;

/**
 * Relay for ayynoxs map pack: what one Horizon player shares (waypoints,
 * explored map tiles) goes to the other players on the server.
 *
 * <p>Horizon's mod messages travel on the plugin channel {@code horizon:mod_net}:
 * a VarInt-prefixed UTF-8 channel name ({@code ayynoxs_map:sync}), then the
 * VarInt-prefixed message bytes. The first byte of a message is its kind:
 * HELLO (1) is answered with WELCOME (2), every other kind is passed on unchanged.
 */
public final class AyynoxsMapSync extends JavaPlugin implements PluginMessageListener, Listener {
    private static final String TRANSPORT = "horizon:mod_net";
    private static final String CHANNEL = "ayynoxs_map:sync";
    private static final byte HELLO = 1;
    private static final byte WELCOME = 2;
    /** Messages a player may send per second; the rest are dropped. */
    private static final int PER_SECOND = 40;

    private final Map<UUID, long[]> rate = new HashMap<>();

    @Override
    public void onEnable() {
        getServer().getMessenger().registerOutgoingPluginChannel(this, TRANSPORT);
        getServer().getMessenger().registerIncomingPluginChannel(this, TRANSPORT, this);
        getServer().getPluginManager().registerEvents(this, this);
        getLogger().info("Relaying ayynoxs map pack shares on " + TRANSPORT);
    }

    @Override
    public void onDisable() {
        getServer().getMessenger().unregisterOutgoingPluginChannel(this);
        getServer().getMessenger().unregisterIncomingPluginChannel(this);
    }

    @EventHandler
    public void onQuit(PlayerQuitEvent event) {
        rate.remove(event.getPlayer().getUniqueId());
    }

    @Override
    public void onPluginMessageReceived(String transport, Player sender, byte[] payload) {
        if (!TRANSPORT.equals(transport) || !allowed(sender)) {
            return;
        }
        byte[] data = unwrap(payload);
        if (data == null || data.length == 0) {
            return;
        }
        if (data[0] == HELLO) {
            sender.sendPluginMessage(this, TRANSPORT, wrap(new byte[] {WELCOME}));
            return;
        }
        for (Player other : getServer().getOnlinePlayers()) {
            if (!other.getUniqueId().equals(sender.getUniqueId())) {
                other.sendPluginMessage(this, TRANSPORT, payload);
            }
        }
    }

    /** At most {@link #PER_SECOND} messages a second from one player. */
    private boolean allowed(Player player) {
        long now = System.currentTimeMillis() / 1000;
        long[] r = rate.computeIfAbsent(player.getUniqueId(), id -> new long[] {now, 0});
        if (r[0] != now) {
            r[0] = now;
            r[1] = 0;
        }
        return ++r[1] <= PER_SECOND;
    }

    /** The message bytes of a payload on our channel, else null. */
    private static byte[] unwrap(byte[] payload) {
        int[] at = {0};
        int len = readVarInt(payload, at);
        if (len < 0 || at[0] + len > payload.length) {
            return null;
        }
        String channel = new String(payload, at[0], len, StandardCharsets.UTF_8);
        at[0] += len;
        if (!CHANNEL.equals(channel)) {
            return null;
        }
        int size = readVarInt(payload, at);
        if (size < 0 || at[0] + size != payload.length) {
            return null;
        }
        byte[] data = new byte[size];
        System.arraycopy(payload, at[0], data, 0, size);
        return data;
    }

    private static byte[] wrap(byte[] data) {
        ByteArrayOutputStream out = new ByteArrayOutputStream();
        byte[] channel = CHANNEL.getBytes(StandardCharsets.UTF_8);
        writeVarInt(out, channel.length);
        out.writeBytes(channel);
        writeVarInt(out, data.length);
        out.writeBytes(data);
        return out.toByteArray();
    }

    private static int readVarInt(byte[] b, int[] at) {
        int value = 0;
        for (int i = 0; i < 5; i++) {
            if (at[0] >= b.length) {
                return -1;
            }
            int v = b[at[0]++];
            value |= (v & 0x7F) << (7 * i);
            if ((v & 0x80) == 0) {
                return value;
            }
        }
        return -1;
    }

    private static void writeVarInt(ByteArrayOutputStream out, int value) {
        while ((value & ~0x7F) != 0) {
            out.write((value & 0x7F) | 0x80);
            value >>>= 7;
        }
        out.write(value);
    }
}
