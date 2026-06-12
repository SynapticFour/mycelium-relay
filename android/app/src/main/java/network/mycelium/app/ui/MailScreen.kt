// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
package network.mycelium.app.ui

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.navigation.NavController
import network.mycelium.app.viewmodel.MailViewModel

@Composable
fun MailScreen(navController: NavController, vm: MailViewModel = viewModel()) {
    val inbox by vm.inbox.collectAsState()
    Scaffold(topBar = { TopAppBar(title = { Text("Mesh Mail") }) }) { padding ->
        LazyColumn(modifier = Modifier.fillMaxSize().padding(padding)) {
            items(inbox) { mail ->
                Text("${mail.fromDisplayName}: ${mail.subject}", modifier = Modifier.padding(16.dp, 8.dp))
            }
        }
    }
}
