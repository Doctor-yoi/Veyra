// This file is created by Veyra and will not be overwritten

package com.example.schedule.ui

import androidx.lifecycle.viewModelScope
import java.time.Instant
import java.util.UUID
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

class NotesViewModel(
    private val notes: NoteStore,
) : NotesViewModelBase() {
    override fun start() {
        viewModelScope.launch {
            notes.forCourse(state.value.courseId).collect { items ->
                mutableState.update { it.copy(items = items) }
            }
        }
    }

    override fun add() {
        viewModelScope.launch {
            val current = state.value
            notes.save(Note(id = UUID.randomUUID().toString(), courseId = current.courseId, text = current.draft, pinned = false, createdAt = Instant.now()))
            mutableState.update { it.copy(draft = "") }
        }
    }

    override fun remove(id: String) {
        viewModelScope.launch { notes.delete(id) }
    }
}
