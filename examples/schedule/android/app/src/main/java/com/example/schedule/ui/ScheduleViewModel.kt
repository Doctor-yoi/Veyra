// This file is created by Veyra and will not be overwritten

package com.example.schedule.ui

import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

class ScheduleViewModel(
    private val courses: CourseRepository,
) : ScheduleViewModelBase() {
    override fun refresh() {
        viewModelScope.launch {
            mutableState.update { it.copy(isRefreshing = true) }
            val result = runCatching { courses.getCourses() }
            mutableState.update { current ->
                val query = current.query
                result.fold(
                    onSuccess = { all ->
                        current.copy(
                            items = if (query.isEmpty()) all else all.filter { it.name.contains(query, ignoreCase = true) },
                            errorMessage = null,
                            isRefreshing = false,
                        )
                    },
                    onFailure = { current.copy(errorMessage = it.message, isRefreshing = false) },
                )
            }
        }
    }

    override fun openCourse(id: String) {
        // Navigation is handled by the app
    }
}
