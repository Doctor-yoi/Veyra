package com.example.schedule

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import com.example.schedule.data.AppDependencies
import com.example.schedule.ui.LocalVeyraDependencies
import com.example.schedule.ui.ScheduleEffect
import com.example.schedule.ui.ScheduleRoute

class MainActivity : ComponentActivity() {
    private val dependencies by lazy { AppDependencies(applicationContext) }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            MaterialTheme {
                CompositionLocalProvider(LocalVeyraDependencies provides dependencies) {
                    // ScheduleViewModel发出showCourse后显示详情，返回键回到列表
                    var selected by rememberSaveable { mutableStateOf<String?>(null) }
                    val id = selected
                    if (id == null) {
                        ScheduleRoute(onEffect = { effect ->
                            when (effect) {
                                is ScheduleEffect.ShowCourse -> selected = effect.id
                            }
                        })
                    } else {
                        BackHandler { selected = null }
                        Text(text = "Course $id")
                    }
                }
            }
        }
    }
}
