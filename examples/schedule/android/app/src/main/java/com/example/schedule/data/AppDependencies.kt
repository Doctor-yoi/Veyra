package com.example.schedule.data

import android.content.Context
import com.example.schedule.ui.Account
import com.example.schedule.ui.AppPreferences
import com.example.schedule.ui.Course
import android.content.Context
import com.example.schedule.ui.Account
import com.example.schedule.ui.AppPreferences
import com.example.schedule.ui.CourseKind
import android.content.Context
import com.example.schedule.ui.Account
import com.example.schedule.ui.AppPreferences
import com.example.schedule.ui.CourseRepository
import com.example.schedule.ui.DataStoreAppPreferences
import com.example.schedule.ui.KeystoreAccount
import com.example.schedule.ui.NoteStore
import com.example.schedule.ui.VeyraDatabase
import com.example.schedule.ui.VeyraStores
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

class AppDependencies(context: Context) : VeyraDependencies {
    private val stores = VeyraStores(VeyraDatabase.build(context))

    override val courseRepository: CourseRepository = InMemoryCourseRepository()
    override val noteStore: NoteStore = stores.noteStore
    override val appPreferences: AppPreferences = DataStoreAppPreferences(context)
    override val account: Account = KeystoreAccount(context)
}
