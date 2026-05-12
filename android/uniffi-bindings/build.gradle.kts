plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "network.mycelium.bindings"
    compileSdk = 34
    defaultConfig {
        minSdk = 26
    }
    kotlinOptions {
        jvmTarget = "17"
    }
}

dependencies {
    implementation("net.java.dev.jna:jna:5.14.0")
}
