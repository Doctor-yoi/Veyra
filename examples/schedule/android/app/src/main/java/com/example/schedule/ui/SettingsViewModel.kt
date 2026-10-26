// This file is created by Veyra and will not be overwritten

package com.example.schedule.ui

import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

class SettingsViewModel : SettingsViewModelBase() {
    override fun setNickname(value: String) {
        // Keep nicknames short
        mutableState.update { it.copy(nickname = value.take(20)) }
    }

    override fun save() {
        viewModelScope.launch {
            // Persist settings here
        }
    }
}
