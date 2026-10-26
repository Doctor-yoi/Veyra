package com.example.schedule

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.CompositionLocalProvider
import com.example.schedule.data.AppDependencies
import com.example.schedule.ui.LocalVeyraDependencies
import com.example.schedule.ui.ScheduleRoute

class MainActivity : ComponentActivity() {
    private val dependencies by lazy { AppDependencies(applicationContext) }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            MaterialTheme {
                CompositionLocalProvider(LocalVeyraDependencies provides dependencies) {
                    ScheduleRoute()
                }
            }
        }
    }
}
