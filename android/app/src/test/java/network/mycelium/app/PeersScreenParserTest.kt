package network.mycelium.app

import network.mycelium.app.ui.parseScannedPeerInfo
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class PeersScreenParserTest {
    @Test
    fun parse_peer_and_host_format_to_multiaddr() {
        val parsed = parseScannedPeerInfo("12D3KooWabc@192.168.1.4:7761")
        assertEquals("/ip4/192.168.1.4/tcp/7761/p2p/12D3KooWabc", parsed)
    }

    @Test
    fun passthrough_full_multiaddr() {
        val input = "/ip4/192.168.1.4/tcp/7761/p2p/12D3KooWabc"
        assertEquals(input, parseScannedPeerInfo(input))
    }

    @Test
    fun reject_invalid_payload() {
        assertNull(parseScannedPeerInfo("not-a-peer-qr"))
    }
}
