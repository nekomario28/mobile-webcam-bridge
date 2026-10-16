plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

val appVersion = rootProject.file("../VERSION").readText().trim()
val appVersionParts = Regex("^(\\d+)\\.(\\d+)\\.(\\d+)$")
    .matchEntire(appVersion)?.groupValues?.drop(1)?.map(String::toInt)
    ?: error("VERSION must use major.minor.patch")
val releaseStoreFile = providers.environmentVariable("AMB_RELEASE_STORE_FILE").orNull
val releaseStorePassword = providers.environmentVariable("AMB_RELEASE_STORE_PASSWORD").orNull
val releaseKeyAlias = providers.environmentVariable("AMB_RELEASE_KEY_ALIAS").orNull
val releaseKeyPassword = providers.environmentVariable("AMB_RELEASE_KEY_PASSWORD").orNull
val releaseSigningAvailable = listOf(
    releaseStoreFile, releaseStorePassword, releaseKeyAlias, releaseKeyPassword
).all { !it.isNullOrBlank() }
val partialReleaseSigning = listOf(
    releaseStoreFile, releaseStorePassword, releaseKeyAlias, releaseKeyPassword
).any { !it.isNullOrBlank() } && !releaseSigningAvailable
check(!partialReleaseSigning) { "Set all AMB_RELEASE_* signing variables or none of them" }

android {
    namespace = "dev.nekomario.amb"
    compileSdk = 35

    // Tests use the same public wire/signature fixtures as Rust and JVM tests,
    // packaged only in the instrumentation APK, not the production app.
    sourceSets.getByName("androidTest").assets.srcDir(rootProject.file("../protocol/fixtures"))

    defaultConfig {
        applicationId = "dev.nekomario.amb"
        minSdk = 26
        targetSdk = 35
        testInstrumentationRunner = "dev.nekomario.amb.UsbPermissionInstrumentation"
        versionCode = appVersionParts[0] * 1_000_000 +
            appVersionParts[1] * 1_000 + appVersionParts[2]
        versionName = appVersion
    }

    signingConfigs {
        if (releaseSigningAvailable) {
            create("release") {
                storeFile = file(releaseStoreFile!!)
                storePassword = releaseStorePassword
                keyAlias = releaseKeyAlias
                keyPassword = releaseKeyPassword
            }
        }
    }

    buildTypes {
        getByName("release") {
            if (releaseSigningAvailable) signingConfig = signingConfigs.getByName("release")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

kotlin {
    jvmToolchain(17)
}

// CI failure evidence: the log truncates JUnit failures to
// "Exception at file:line", so keep the full assertion messages and the
// test's own stdout/stderr in the build output, and let CI upload the
// reports as an artifact (android-ci).
tasks.withType<Test> {
    testLogging {
        showStandardStreams = true
        exceptionFormat = org.gradle.api.tasks.testing.logging.TestExceptionFormat.FULL
        showStackTraces = true
    }
}

dependencies {
    testImplementation("junit:junit:4.13.2")
}
