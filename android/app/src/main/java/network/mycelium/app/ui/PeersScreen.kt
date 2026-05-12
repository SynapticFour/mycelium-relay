package network.mycelium.app.ui

import android.graphics.Bitmap
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Button
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.unit.dp
import androidx.core.graphics.createBitmap
import com.google.zxing.BarcodeFormat
import com.google.zxing.qrcode.QRCodeWriter
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.navigation.NavController
import com.journeyapps.barcodescanner.ScanContract
import com.journeyapps.barcodescanner.ScanOptions
import network.mycelium.app.viewmodel.ChatViewModel
import network.mycelium.bindings.mycelium

@Composable
fun PeersScreen(navController: NavController, vm: ChatViewModel = viewModel()) {
    val peers by vm.peers.collectAsState()
    val localPayload = "${mycelium.localPeerId()}@0.0.0.0:7761"
    val bootstrap = remember { mutableStateOf("") }
    var scanError by remember { mutableStateOf<String?>(null) }
    val qrBitmap = remember(localPayload) { generateQrCode(localPayload, 512) }
    val scannerLauncher = rememberLauncherForActivityResult(ScanContract()) { result ->
        val raw = result.contents ?: return@rememberLauncherForActivityResult
        val multiaddr = parseScannedPeerInfo(raw)
        if (multiaddr == null) {
            scanError = "QR-Format ungültig"
            return@rememberLauncherForActivityResult
        }
        mycelium.addBootstrapPeer(multiaddr)
        scanError = null
    }
    Scaffold(topBar = { TopAppBar(title = { Text("Peers") }) }) { padding ->
        Column(modifier = Modifier.fillMaxSize().padding(padding)) {
            Text("Peers: ${peers.size}", modifier = Modifier.padding(16.dp))
            Image(
                bitmap = qrBitmap.asImageBitmap(),
                contentDescription = "Meine Peer-ID als QR-Code",
                modifier = Modifier.padding(horizontal = 16.dp).size(180.dp),
            )
            Text(
                text = localPayload,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
                color = Color.Gray,
            )
            Row(
                modifier = Modifier.padding(16.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Button(onClick = {
                    scannerLauncher.launch(
                        ScanOptions()
                            .setDesiredBarcodeFormats(ScanOptions.QR_CODE)
                            .setPrompt("Peer-QR scannen")
                            .setBeepEnabled(false)
                            .setOrientationLocked(false),
                    )
                }) {
                    Text("Scan QR")
                }
                TextField(
                    value = bootstrap.value,
                    onValueChange = { bootstrap.value = it },
                    modifier = Modifier.weight(1f),
                    placeholder = { Text("/ip4/192.168.1.5/tcp/7761/p2p/...") },
                )
                Button(onClick = {
                    if (bootstrap.value.isNotBlank()) {
                        mycelium.addBootstrapPeer(bootstrap.value.trim())
                        bootstrap.value = ""
                    }
                }) {
                    Text("Add")
                }
            }
            if (scanError != null) {
                Text(
                    text = scanError!!,
                    modifier = Modifier.padding(horizontal = 16.dp),
                    color = Color.Red,
                )
            }
            peers.forEach { peer -> Text(peer, modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp)) }
        }
    }
}

internal fun generateQrCode(payload: String, size: Int): Bitmap {
    val matrix = QRCodeWriter().encode(payload, BarcodeFormat.QR_CODE, size, size)
    val bitmap = createBitmap(size, size)
    for (x in 0 until size) {
        for (y in 0 until size) {
            bitmap.setPixel(x, y, if (matrix[x, y]) android.graphics.Color.BLACK else android.graphics.Color.WHITE)
        }
    }
    return bitmap
}

fun parseScannedPeerInfo(raw: String): String? {
    val input = raw.trim()
    if (input.startsWith("/ip4/") || input.startsWith("/dns/")) {
        return input
    }
    val parts = input.split("@")
    if (parts.size != 2) {
        return null
    }
    val peerId = parts[0]
    val hostAndPort = parts[1].split(":")
    if (peerId.isBlank() || hostAndPort.size != 2) {
        return null
    }
    val host = hostAndPort[0]
    val port = hostAndPort[1].toIntOrNull() ?: return null
    return "/ip4/$host/tcp/$port/p2p/$peerId"
}
