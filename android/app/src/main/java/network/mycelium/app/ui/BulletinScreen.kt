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
import network.mycelium.app.viewmodel.BulletinViewModel

@Composable
fun BulletinScreen(navController: NavController, vm: BulletinViewModel = viewModel()) {
    val posts by vm.posts.collectAsState()
    Scaffold(topBar = { TopAppBar(title = { Text("Bulletin") }) }) { padding ->
        LazyColumn(modifier = Modifier.fillMaxSize().padding(padding)) {
            items(posts) { post ->
                Text("${post.title} (${post.scope})", modifier = Modifier.padding(16.dp, 8.dp))
                Text(post.body, modifier = Modifier.padding(horizontal = 16.dp))
            }
        }
    }
}
