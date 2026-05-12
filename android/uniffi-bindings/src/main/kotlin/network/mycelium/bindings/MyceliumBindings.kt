package network.mycelium.bindings

typealias NodeConfig = uniffi.mycelium.NodeConfig
typealias ChatMessage = uniffi.mycelium.ChatMessage
typealias BulletinPost = uniffi.mycelium.BulletinPost
typealias MailMessage = uniffi.mycelium.MailMessage
typealias NodeMetrics = uniffi.mycelium.NodeMetrics
typealias WalletBalance = uniffi.mycelium.WalletBalance
typealias TxInfo = uniffi.mycelium.TxInfo
typealias EnergyState = uniffi.mycelium.EnergyState
typealias NodeEventCallback = uniffi.mycelium.NodeEventCallback
typealias ConnectivityMode = uniffi.mycelium.ConnectivityMode
typealias HotWalletConfig = uniffi.mycelium.HotWalletConfig
typealias HotWalletStatus = uniffi.mycelium.HotWalletStatus
typealias PaymentRequestData = uniffi.mycelium.PaymentRequestData

object mycelium {
    fun initNode(config: NodeConfig) = uniffi.mycelium.initNode(config)
    fun stopNode() = uniffi.mycelium.stopNode()
    fun localPeerId(): String = uniffi.mycelium.localPeerId()
    fun knownPeers(): List<String> = uniffi.mycelium.knownPeers()
    fun metrics(): NodeMetrics = uniffi.mycelium.metrics()
    fun sendChatDirect(toPeer: String, body: String) = uniffi.mycelium.sendChatDirect(toPeer, body)
    fun sendChatBroadcast(body: String) = uniffi.mycelium.sendChatBroadcast(body)
    fun chatHistory(peerId: String, limit: UInt): List<ChatMessage> = uniffi.mycelium.chatHistory(peerId, limit)
    fun postBulletin(scope: String, title: String, body: String, ttlSecs: ULong) =
        uniffi.mycelium.postBulletin(scope, title, body, ttlSecs)
    fun bulletinsForScope(scope: String): List<BulletinPost> = uniffi.mycelium.bulletinsForScope(scope)
    fun sendMail(toPeer: String, subject: String, body: String) = uniffi.mycelium.sendMail(toPeer, subject, body)
    fun mailInbox(limit: UInt): List<MailMessage> = uniffi.mycelium.mailInbox(limit)
    fun mailSent(limit: UInt): List<MailMessage> = uniffi.mycelium.mailSent(limit)
    fun markMailRead(mailId: String) = uniffi.mycelium.markMailRead(mailId)
    fun setDisplayName(name: String) = uniffi.mycelium.setDisplayName(name)
    fun displayName(): String = uniffi.mycelium.displayName()
    fun setEnergyState(state: EnergyState) = uniffi.mycelium.setEnergyState(state)
    fun addBootstrapPeer(multiaddr: String) = uniffi.mycelium.addBootstrapPeer(multiaddr)
    fun setEventCallback(callback: NodeEventCallback) = uniffi.mycelium.setEventCallback(callback)
    fun walletAddress(): String = uniffi.mycelium.walletAddress()
    fun walletBalance(): WalletBalance = uniffi.mycelium.walletBalance()
    fun sendTransaction(toAddress: String, amountMuon: ULong, memo: String?) =
        uniffi.mycelium.sendTransaction(toAddress, amountMuon, memo)
    fun walletRecentTransactions(limit: UInt): List<TxInfo> =
        uniffi.mycelium.walletRecentTransactions(limit)
    fun requestFaucet() = uniffi.mycelium.requestFaucet()
    fun currentConnectivityMode(): ConnectivityMode = uniffi.mycelium.currentConnectivityMode()
    fun getHotWalletConfig(): HotWalletConfig = uniffi.mycelium.getHotWalletConfig()
    fun setHotWalletConfig(config: HotWalletConfig) = uniffi.mycelium.setHotWalletConfig(config)
    fun setColdWalletAddress(address: String?) = uniffi.mycelium.setColdWalletAddress(address)
    fun hotWalletStatus(): HotWalletStatus = uniffi.mycelium.hotWalletStatus()
    fun buildPaymentRequestUri(toAddress: String, amountMuon: ULong, memo: String?): String =
        uniffi.mycelium.buildPaymentRequestUri(toAddress, amountMuon, memo)
    fun parsePaymentRequestUri(uri: String): PaymentRequestData? =
        uniffi.mycelium.parsePaymentRequestUri(uri)
}
