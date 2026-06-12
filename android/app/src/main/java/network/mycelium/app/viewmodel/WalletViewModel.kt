// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
package network.mycelium.app.viewmodel

import android.content.Context
import android.content.Intent
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import network.mycelium.bindings.ConnectivityMode
import network.mycelium.bindings.HotWalletConfig
import network.mycelium.bindings.HotWalletStatus
import network.mycelium.bindings.TxInfo
import network.mycelium.bindings.mycelium

class WalletViewModel : ViewModel() {
    private val _balance = MutableStateFlow(0L)
    val balance: StateFlow<Long> = _balance.asStateFlow()
    private val _pending = MutableStateFlow(0L)
    val pending: StateFlow<Long> = _pending.asStateFlow()
    private val _transactions = MutableStateFlow<List<TxInfo>>(emptyList())
    val recentTransactions: StateFlow<List<TxInfo>> = _transactions.asStateFlow()
    private val _localAddress = MutableStateFlow("")
    val localAddress: StateFlow<String> = _localAddress.asStateFlow()

    private val _showSendDialog = MutableStateFlow(false)
    val showSendDialog: StateFlow<Boolean> = _showSendDialog.asStateFlow()
    private val _showReceiveDialog = MutableStateFlow(false)
    val showReceiveDialog: StateFlow<Boolean> = _showReceiveDialog.asStateFlow()

    private val _sendPrefill = MutableStateFlow(Triple("", "", ""))
    val sendPrefill: StateFlow<Triple<String, String, String>> = _sendPrefill.asStateFlow()

    private val _connectivity = MutableStateFlow(ConnectivityMode.INTERNET)
    val connectivity: StateFlow<ConnectivityMode> = _connectivity.asStateFlow()
    private val _hotWalletConfig = MutableStateFlow<HotWalletConfig?>(null)
    val hotWalletConfig: StateFlow<HotWalletConfig?> = _hotWalletConfig.asStateFlow()
    private val _hotWalletStatus = MutableStateFlow<HotWalletStatus?>(null)
    val hotWalletStatus: StateFlow<HotWalletStatus?> = _hotWalletStatus.asStateFlow()

    init {
        refresh()
    }

    fun refresh() {
        viewModelScope.launch(Dispatchers.IO) {
            runCatching {
                val b = mycelium.walletBalance()
                _balance.value = b.confirmedMuon.toLong()
                _pending.value = b.pendingMuon.toLong()
                _localAddress.value = mycelium.walletAddress()
                _transactions.value = mycelium.walletRecentTransactions(50u)
                _connectivity.value = mycelium.currentConnectivityMode()
                _hotWalletConfig.value = mycelium.getHotWalletConfig()
                _hotWalletStatus.value = mycelium.hotWalletStatus()
            }
        }
    }

    fun openSendDialog() {
        _sendPrefill.value = Triple("", "", "")
        _showSendDialog.value = true
    }

    fun closeSendDialog() {
        _showSendDialog.value = false
    }

    fun openReceiveDialog() {
        _showReceiveDialog.value = true
    }

    fun closeReceiveDialog() {
        _showReceiveDialog.value = false
    }

    fun requestFaucet() {
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { mycelium.requestFaucet() }
            refresh()
        }
    }

    fun sendTransaction(toAddress: String, amountMxcText: String, memo: String?) {
        viewModelScope.launch(Dispatchers.IO) {
            val mxc = amountMxcText.toDoubleOrNull() ?: 0.0
            val muon = (mxc * 1_000_000.0).toLong().coerceAtLeast(0)
            if (muon > 0 && toAddress.isNotBlank()) {
                runCatching {
                    mycelium.sendTransaction(toAddress.trim(), muon.toULong(), memo?.ifBlank { null })
                }
            }
            refresh()
        }
    }

    fun openSendWithPaymentRequest(pr: network.mycelium.bindings.PaymentRequestData) {
        viewModelScope.launch(Dispatchers.Main) {
            val amt = (pr.amountMuon.toDouble() / 1_000_000.0).toString()
            _sendPrefill.value = Triple(pr.toAddress, amt, pr.memo.orEmpty())
            _showSendDialog.value = true
        }
    }

    fun updateCacheLimitMxc(mxc: Float) {
        viewModelScope.launch(Dispatchers.IO) {
            runCatching {
                val maxMuon = (mxc * 1_000_000.0).toLong().coerceAtLeast(0).toULong()
                val thresh = (maxMuon.toDouble() * 0.8).toULong()
                val c = mycelium.getHotWalletConfig()
                mycelium.setHotWalletConfig(
                    HotWalletConfig(
                        maxCacheMuon = maxMuon,
                        refillThresholdMuon = thresh,
                        refillAmountMuon = maxMuon,
                        coldWalletAddress = c.coldWalletAddress,
                    ),
                )
                refresh()
            }
        }
    }

    fun setColdWalletAddress(addr: String?) {
        viewModelScope.launch(Dispatchers.IO) {
            runCatching {
                mycelium.setColdWalletAddress(addr?.trim()?.ifBlank { null })
                refresh()
            }
        }
    }

    fun toggleAutoRefill(enabled: Boolean) {
        viewModelScope.launch(Dispatchers.IO) {
            runCatching {
                if (!enabled) {
                    mycelium.setColdWalletAddress(null)
                }
                refresh()
            }
        }
    }

    fun sharePaymentUri(context: Context, uri: String) {
        val send = Intent(Intent.ACTION_SEND).apply {
            type = "text/plain"
            putExtra(Intent.EXTRA_TEXT, uri)
        }
        context.startActivity(Intent.createChooser(send, null))
    }

    fun buildMxcpayUri(amountMxcText: String, memo: String): String? {
        val mxc = amountMxcText.toDoubleOrNull() ?: return null
        val muon = (mxc * 1_000_000.0).toLong().coerceAtLeast(0).toULong()
        if (muon == 0uL) return null
        val addr = mycelium.walletAddress()
        return mycelium.buildPaymentRequestUri(addr, muon, memo.ifBlank { null })
    }
}
