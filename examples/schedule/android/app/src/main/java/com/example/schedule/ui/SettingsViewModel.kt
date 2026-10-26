// This file is created by Veyra and will not be overwritten

package com.example.schedule.ui

import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

class SettingsViewModel(
    private val preferences: AppPreferences,
    private val account: Account,
) : SettingsViewModelBase() {
    override fun setNickname(value: String) {
        // Keep nicknames short
        mutableState.update { it.copy(nickname = value.take(20)) }
    }

    override fun load() {
        viewModelScope.launch {
            val reminder = preferences.reminder.first()
            val volume = preferences.volume.first()
            val nickname = preferences.nickname.first()
            val signedIn = account.token() != null
            mutableState.update { it.copy(reminder = reminder, volume = volume, nickname = nickname, signedIn = signedIn) }
        }
    }

    override fun save() {
        viewModelScope.launch {
            val current = state.value
            preferences.setReminder(current.reminder)
            preferences.setVolume(current.volume)
            preferences.setNickname(current.nickname)
        }
    }

    override fun signOut() {
        viewModelScope.launch {
            account.setToken(null)
            mutableState.update { it.copy(signedIn = false) }
        }
    }
}
