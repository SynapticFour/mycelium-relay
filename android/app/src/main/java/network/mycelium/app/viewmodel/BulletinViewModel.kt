// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
package network.mycelium.app.viewmodel

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.stateIn
import network.mycelium.bindings.BulletinPost
import network.mycelium.bindings.mycelium

class BulletinViewModel(application: Application) : AndroidViewModel(application) {
    val posts: StateFlow<List<BulletinPost>> =
        flow {
            while (true) {
                emit(mycelium.bulletinsForScope("mycelium/global"))
                delay(3_000)
            }
        }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), emptyList())
}
