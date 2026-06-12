// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
package network.mycelium.app.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Send
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.viewmodel.compose.viewModel
import network.mycelium.app.viewmodel.ChatViewModel

@Composable
fun ChatConversationScreen(peerId: String, vm: ChatViewModel = viewModel()) {
    val messages by vm.chatHistory(peerId).collectAsState(emptyList())
    var input by remember { mutableStateOf("") }

    Column {
        LazyColumn(modifier = Modifier.weight(1f, fill = true)) {
            items(messages) { msg ->
                Text("${msg.fromDisplayName}: ${msg.body}", modifier = Modifier.padding(8.dp))
            }
        }
        Row(modifier = Modifier.fillMaxWidth().padding(8.dp)) {
            TextField(
                value = input,
                onValueChange = { input = it },
                modifier = Modifier.weight(1f),
                placeholder = { Text("Nachricht...") },
            )
            IconButton(onClick = {
                vm.sendChat(peerId, input)
                input = ""
            }) {
                Icon(Icons.Default.Send, contentDescription = "Senden")
            }
        }
    }
}
