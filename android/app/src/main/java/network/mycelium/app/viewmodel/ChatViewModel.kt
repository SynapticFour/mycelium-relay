package network.mycelium.app.viewmodel

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import network.mycelium.bindings.ChatMessage
import network.mycelium.bindings.mycelium

class ChatViewModel(application: Application) : AndroidViewModel(application) {
    val localPeerId: String get() = mycelium.localPeerId()

    val peers: StateFlow<List<String>> =
        flow {
            while (true) {
                emit(mycelium.knownPeers())
                delay(2_000)
            }
        }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), emptyList())

    fun chatHistory(peerId: String): Flow<List<ChatMessage>> =
        flow {
            while (true) {
                emit(mycelium.chatHistory(peerId, 100u))
                delay(1_000)
            }
        }

    fun sendChat(toPeer: String, body: String) {
        viewModelScope.launch(Dispatchers.IO) {
            mycelium.sendChatDirect(toPeer, body)
        }
    }
}
