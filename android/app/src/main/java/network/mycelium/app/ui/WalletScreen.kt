// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
package network.mycelium.app.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.ArrowDownward
import androidx.compose.material.icons.filled.ArrowUpward
import androidx.compose.material.icons.filled.QrCode
import androidx.compose.material.icons.filled.QrCodeScanner
import androidx.compose.material.icons.filled.Send
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.IconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Slider
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.ui.unit.dp
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.navigation.NavController
import com.journeyapps.barcodescanner.ScanContract
import com.journeyapps.barcodescanner.ScanOptions
import java.util.Locale
import network.mycelium.app.R
import network.mycelium.app.viewmodel.WalletViewModel
import network.mycelium.bindings.ConnectivityMode
import network.mycelium.bindings.HotWalletConfig
import network.mycelium.bindings.HotWalletStatus
import network.mycelium.bindings.TxInfo
import network.mycelium.bindings.mycelium

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun WalletScreen(navController: NavController, vm: WalletViewModel = viewModel()) {
    val balance by vm.balance.collectAsState()
    val pending by vm.pending.collectAsState()
    val transactions by vm.recentTransactions.collectAsState()
    val localAddress by vm.localAddress.collectAsState()
    val showSend by vm.showSendDialog.collectAsState()
    val showReceive by vm.showReceiveDialog.collectAsState()
    val sendPrefill by vm.sendPrefill.collectAsState()
    val connectivity by vm.connectivity.collectAsState()
    val hotCfg by vm.hotWalletConfig.collectAsState()
    val hotStatus by vm.hotWalletStatus.collectAsState()

    DisposableEffect(Unit) {
        vm.refresh()
        onDispose { }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.wallet_title)) },
                navigationIcon = {
                    IconButton(onClick = { navController.popBackStack() }) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = null)
                    }
                },
            )
        },
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding),
        ) {
            Card(modifier = Modifier.fillMaxWidth().padding(16.dp)) {
                Column(modifier = Modifier.padding(24.dp)) {
                    Text(
                        text = formatMxc(balance),
                        style = MaterialTheme.typography.headlineLarge,
                        fontWeight = FontWeight.Bold,
                    )
                    Text(
                        "MXC",
                        style = MaterialTheme.typography.titleMedium,
                        color = MaterialTheme.colorScheme.primary,
                    )
                    if (pending > 0) {
                        Text(
                            text = "+ ${formatMxc(pending)} ${stringResource(R.string.wallet_pending)}",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.secondary,
                        )
                    }
                }
            }
            Row(
                modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Button(
                    modifier = Modifier.weight(1f),
                    onClick = { vm.openSendDialog() },
                ) {
                    Icon(Icons.Default.Send, contentDescription = null)
                    Spacer(Modifier.width(8.dp))
                    Text(stringResource(R.string.wallet_send))
                }
                OutlinedButton(
                    modifier = Modifier.weight(1f),
                    onClick = { vm.openReceiveDialog() },
                ) {
                    Icon(Icons.Default.QrCode, contentDescription = null)
                    Spacer(Modifier.width(8.dp))
                    Text(stringResource(R.string.wallet_receive))
                }
            }
            OutlinedButton(
                onClick = { vm.requestFaucet() },
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 16.dp),
            ) {
                Text(stringResource(R.string.wallet_faucet))
            }
            if (hotCfg != null && hotStatus != null) {
                HotWalletCard(
                    connectivity = connectivity,
                    cfg = hotCfg!!,
                    status = hotStatus!!,
                    onCacheMxcChanged = { vm.updateCacheLimitMxc(it) },
                    onColdAddressCommit = { vm.setColdWalletAddress(it) },
                    onAutoRefill = { vm.toggleAutoRefill(it) },
                )
            }
            Text(
                stringResource(R.string.wallet_history),
                style = MaterialTheme.typography.titleMedium,
                modifier = Modifier.padding(16.dp, 8.dp),
            )
            LazyColumn(modifier = Modifier.fillMaxWidth()) {
                items(transactions) { tx ->
                    TransactionItem(tx = tx, localAddress = localAddress)
                }
                if (transactions.isEmpty()) {
                    item {
                        Text(
                            stringResource(R.string.wallet_no_txs),
                            modifier = Modifier.padding(16.dp),
                            color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.5f),
                        )
                    }
                }
            }
        }
    }

    if (showSend) {
        SendTransactionDialog(
            prefill = sendPrefill,
            onSend = { to, amount, memo ->
                vm.sendTransaction(to, amount, memo)
                vm.closeSendDialog()
            },
            onDismiss = { vm.closeSendDialog() },
            onScanMxcpay = { pr -> vm.openSendWithPaymentRequest(pr) },
        )
    }

    if (showReceive) {
        ReceivePaymentDialog(
            onDismiss = { vm.closeReceiveDialog() },
            vm = vm,
        )
    }
}

@Composable
private fun HotWalletCard(
    connectivity: ConnectivityMode,
    cfg: HotWalletConfig,
    status: HotWalletStatus,
    onCacheMxcChanged: (Float) -> Unit,
    onColdAddressCommit: (String?) -> Unit,
    onAutoRefill: (Boolean) -> Unit,
) {
    var coldInput by remember { mutableStateOf(cfg.coldWalletAddress.orEmpty()) }
    LaunchedEffect(cfg.coldWalletAddress) {
        coldInput = cfg.coldWalletAddress.orEmpty()
    }
    val maxMxc = (cfg.maxCacheMuon.toDouble() / 1_000_000.0).toFloat().coerceIn(5f, 500f)
    Card(modifier = Modifier.fillMaxWidth().padding(16.dp)) {
        Column(modifier = Modifier.padding(16.dp)) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    stringResource(R.string.wallet_hot_wallet),
                    style = MaterialTheme.typography.titleMedium,
                )
                Surface(
                    color = if (connectivity == ConnectivityMode.INTERNET) {
                        MaterialTheme.colorScheme.primaryContainer
                    } else {
                        MaterialTheme.colorScheme.secondaryContainer
                    },
                    shape = MaterialTheme.shapes.small,
                ) {
                    Text(
                        text = if (connectivity == ConnectivityMode.INTERNET) {
                            stringResource(R.string.connectivity_internet)
                        } else {
                            stringResource(R.string.connectivity_mesh)
                        },
                        modifier = Modifier.padding(horizontal = 8.dp, vertical = 4.dp),
                        style = MaterialTheme.typography.labelSmall,
                    )
                }
            }
            Spacer(Modifier.height(8.dp))
            Text(
                stringResource(R.string.wallet_cache_limit, formatMxc(cfg.maxCacheMuon.toLong())),
                style = MaterialTheme.typography.bodySmall,
            )
            Slider(
                value = maxMxc,
                onValueChange = { onCacheMxcChanged(it) },
                valueRange = 5f..500f,
                steps = 98,
                modifier = Modifier.fillMaxWidth(),
            )
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    stringResource(R.string.wallet_auto_refill),
                    style = MaterialTheme.typography.bodyMedium,
                )
                Switch(
                    checked = status.autoRefillEnabled,
                    onCheckedChange = onAutoRefill,
                )
            }
            OutlinedTextField(
                value = coldInput,
                onValueChange = { coldInput = it },
                label = { Text(stringResource(R.string.wallet_cold_wallet_address)) },
                placeholder = { Text(stringResource(R.string.wallet_cold_wallet_hint)) },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.End,
            ) {
                TextButton(onClick = { onColdAddressCommit(coldInput.ifBlank { null }) }) {
                    Text(stringResource(R.string.wallet_set_cold_wallet))
                }
            }
            if (status.needsRefill && status.autoRefillEnabled) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    CircularProgressIndicator(
                        modifier = Modifier.size(16.dp),
                        strokeWidth = 2.dp,
                    )
                    Spacer(Modifier.width(8.dp))
                    Text(
                        stringResource(R.string.wallet_refilling),
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
            }
        }
    }
}

@Composable
private fun ReceivePaymentDialog(
    onDismiss: () -> Unit,
    vm: WalletViewModel,
) {
    val ctx = LocalContext.current
    var amount by remember { mutableStateOf("") }
    var memo by remember { mutableStateOf("") }
    val uri = remember(amount, memo) { vm.buildMxcpayUri(amount, memo) }
    val qr = remember(uri) {
        uri?.let { generateQrCode(it, 512) }
    }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.wallet_receive_title)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(
                    value = amount,
                    onValueChange = { amount = it.filter { c -> c.isDigit() || c == '.' } },
                    label = { Text(stringResource(R.string.wallet_amount_hint)) },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                )
                OutlinedTextField(
                    value = memo,
                    onValueChange = { memo = it.take(64) },
                    label = { Text(stringResource(R.string.wallet_memo_hint)) },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                )
                qr?.let { bmp ->
                    Image(
                        bitmap = bmp.asImageBitmap(),
                        contentDescription = stringResource(R.string.wallet_payment_qr),
                        modifier = Modifier.size(220.dp).align(Alignment.CenterHorizontally),
                    )
                }
            }
        },
        confirmButton = {
            Row {
                TextButton(
                    onClick = {
                        uri?.let { vm.sharePaymentUri(ctx, it) }
                    },
                    enabled = uri != null,
                ) {
                    Text(stringResource(R.string.wallet_share_request))
                }
                TextButton(onClick = onDismiss) {
                    Text(stringResource(R.string.wallet_dismiss))
                }
            }
        },
    )
}

@Composable
fun TransactionItem(tx: TxInfo, localAddress: String) {
    val isOutgoing = tx.fromAddress == localAddress
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 8.dp),
    ) {
        Icon(
            if (isOutgoing) Icons.Default.ArrowUpward else Icons.Default.ArrowDownward,
            contentDescription = null,
            tint = if (isOutgoing) {
                MaterialTheme.colorScheme.error
            } else {
                MaterialTheme.colorScheme.primary
            },
        )
        Spacer(Modifier.width(12.dp))
        Column(modifier = Modifier.weight(1f)) {
            val peer = if (isOutgoing) tx.toAddress else tx.fromAddress
            Text(
                text = if (isOutgoing) {
                    "→ ${peer.take(12)}…"
                } else {
                    "← ${peer.take(12)}…"
                },
                style = MaterialTheme.typography.bodyMedium,
            )
            tx.memo?.let { Text(it, style = MaterialTheme.typography.bodySmall) }
        }
        Column(horizontalAlignment = Alignment.End) {
            val amt = tx.amountMuon.toLong()
            Text(
                "${if (isOutgoing) "-" else "+"} ${formatMxc(amt)}",
                fontWeight = FontWeight.Medium,
                color = if (isOutgoing) {
                    MaterialTheme.colorScheme.error
                } else {
                    MaterialTheme.colorScheme.primary
                },
            )
            Text(
                text = if (tx.confirmed) {
                    stringResource(R.string.wallet_status_confirmed)
                } else {
                    stringResource(R.string.wallet_status_witnesses, tx.witnessCount.toInt())
                },
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.5f),
            )
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun SendTransactionDialog(
    prefill: Triple<String, String, String>,
    onSend: (String, String, String?) -> Unit,
    onDismiss: () -> Unit,
    onScanMxcpay: (network.mycelium.bindings.PaymentRequestData) -> Unit,
) {
    var to by remember { mutableStateOf("") }
    var amount by remember { mutableStateOf("") }
    var memo by remember { mutableStateOf("") }
    var scanError by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(prefill) {
        to = prefill.first
        amount = prefill.second
        memo = prefill.third
    }

    val scannerLauncher = rememberLauncherForActivityResult(ScanContract()) { result ->
        val raw = result.contents ?: return@rememberLauncherForActivityResult
        val pr = mycelium.parsePaymentRequestUri(raw.trim())
        if (pr == null) {
            scanError = "Not a mxcpay QR"
        } else {
            scanError = null
            onDismiss()
            onScanMxcpay(pr)
        }
    }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.wallet_send_title)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(
                    value = to,
                    onValueChange = { to = it },
                    label = { Text(stringResource(R.string.wallet_to_hint)) },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                )
                OutlinedTextField(
                    value = amount,
                    onValueChange = { amount = it },
                    label = { Text(stringResource(R.string.wallet_amount_hint)) },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                )
                OutlinedTextField(
                    value = memo,
                    onValueChange = { memo = it },
                    label = { Text(stringResource(R.string.mail_subject_hint)) },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                )
                OutlinedButton(
                    onClick = {
                        scannerLauncher.launch(
                            ScanOptions()
                                .setDesiredBarcodeFormats(ScanOptions.QR_CODE)
                                .setPrompt("mxcpay")
                                .setBeepEnabled(false)
                                .setOrientationLocked(false),
                        )
                    },
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Icon(Icons.Default.QrCodeScanner, contentDescription = null)
                    Spacer(Modifier.width(8.dp))
                    Text(stringResource(R.string.wallet_scan_mxcpay))
                }
                scanError?.let {
                    Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall)
                }
            }
        },
        confirmButton = {
            TextButton(
                onClick = {
                    onSend(to.trim(), amount.trim(), memo.ifBlank { null })
                },
            ) {
                Text(stringResource(R.string.wallet_send))
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) {
                Text(stringResource(R.string.wallet_dismiss))
            }
        },
    )
}

fun formatMxc(muon: Long): String {
    val mxc = muon.toDouble() / 1_000_000.0
    return String.format(Locale.US, "%.2f MXC", mxc)
}
