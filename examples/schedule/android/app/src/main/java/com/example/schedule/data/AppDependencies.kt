package com.example.schedule.data

import com.example.schedule.ui.Course
import com.example.schedule.ui.CourseKind
import com.example.schedule.ui.CourseRepository
import com.example.schedule.ui.VeyraDependencies
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flowOf

class InMemoryCourseRepository : CourseRepository {
    private val courses = listOf(
        Course(id = "1", name = "Compilers", teacher = "Aho", credits = 4.0, kind = CourseKind.lecture),
        Course(id = "2", name = "Operating Systems", teacher = null, credits = 3.0, kind = CourseKind.lab),
    )

    override suspend fun getCourses(): List<Course> = courses

    override fun observeCourses(): Flow<List<Course>> = flowOf(courses)
}

class AppDependencies : VeyraDependencies {
    override val courseRepository: CourseRepository = InMemoryCourseRepository()
}
