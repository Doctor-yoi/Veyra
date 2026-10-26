// This file is created by Veyra and will not be overwritten

import Foundation

@MainActor
final class SettingsViewModel: SettingsViewModelContract {
    let state = SettingsState()

    private let preferences: any AppPreferences
    private let account: any Account

    init(preferences: any AppPreferences, account: any Account) {
        self.preferences = preferences
        self.account = account
    }

    func setNickname(_ value: String) {
        // Keep nicknames short
        state.nickname = String(value.prefix(20))
    }

    func load() async {
        state.reminder = preferences.reminder
        state.volume = preferences.volume
        state.nickname = preferences.nickname
        state.signedIn = (try? account.token()) != nil
    }

    func save() async {
        preferences.reminder = state.reminder
        preferences.volume = state.volume
        preferences.nickname = state.nickname
    }

    func signOut() async {
        try? account.setToken(nil)
        state.signedIn = false
    }
}
