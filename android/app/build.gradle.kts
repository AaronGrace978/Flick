plugins { id("com.android.application"); id("org.jetbrains.kotlin.android") }
android {
    namespace = "dev.flick.radar"
    compileSdk = 35
    buildToolsVersion = "35.0.0"
    defaultConfig { applicationId = "dev.flick.radar"; minSdk = 26; targetSdk = 35; versionCode = 3; versionName = "0.2.0-alpha.2" }
    compileOptions { sourceCompatibility = JavaVersion.VERSION_17; targetCompatibility = JavaVersion.VERSION_17 }
    kotlinOptions { jvmTarget = "17" }
}

dependencies { implementation(project(":core")) }
