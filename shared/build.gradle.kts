plugins {
    kotlin("multiplatform")
    kotlin("plugin.serialization") version "2.1.10"
    id("com.android.library")
}

kotlin {
    androidTarget {
        compilations.all {
            kotlinOptions.jvmTarget = "17"
        }
    }
    jvm {
        compilations.all {
            kotlinOptions.jvmTarget = "17"
        }
    }
    listOf(
        iosX64(),
        iosArm64(),
        iosSimulatorArm64()
    ).forEach { target ->
        target.binaries.framework {
            baseName = "OrbitShared"
            isStatic = true
        }
    }
    // Desktop targets: Windows, Linux, macOS (x64 + Apple Silicon)
    mingwX64()
    linuxX64()
    macosX64()
    macosArm64()
    sourceSets {
        val commonMain by getting {
            dependencies {
                implementation("org.kotlinx:kotlinx-serialization-json:1.8.1")
                implementation("org.kotlinx:kotlinx-datetime:0.6.2")
                implementation("org.kotlinx:kotlinx-coroutines-core:1.10.2")
                implementation("io.github.vinceglb:filekit-core:0.10.0")
            }
        }
        val commonTest by getting {
            dependencies {
                implementation(kotlin("test"))
                implementation("org.kotlinx:kotlinx-coroutines-test:1.10.2")
            }
        }
        val androidMain by getting {
            dependencies {
                implementation("androidx.room:room-runtime:2.7.0")
                implementation("androidx.room:room-ktx:2.7.0")
                implementation("androidx.room:room-compiler:2.7.0")
                implementation("io.github.vinceglb:filekit-coil:0.10.0")
            }
        }
        val iosMain by creating {
            dependsOn(commonMain)
        }
        val desktopMain by creating {
            dependsOn(commonMain)
        }
        val jvmMain by creating {
            dependsOn(desktopMain)
        }
        val mingwX64Main by getting { dependsOn(desktopMain) }
        val linuxX64Main by getting { dependsOn(desktopMain) }
        val macosX64Main by getting { dependsOn(desktopMain) }
        val macosArm64Main by getting { dependsOn(desktopMain) }
    }
}

android {
    namespace = "com.orbit.shared"
    compileSdk = 35
    defaultConfig {
        minSdk = 26
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}
