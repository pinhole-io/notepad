import org.jetbrains.kotlin.gradle.dsl.JvmTarget
import java.io.File
import java.util.Properties

fun parseDotEnv(content: String): Map<String, String> {
    val out = mutableMapOf<String, String>()
    for (line in content.lines()) {
        val trimmed = line.trim()
        if (trimmed.isEmpty() || trimmed.startsWith("#")) continue
        val eq = trimmed.indexOf('=')
        if (eq <= 0) continue
        val key = trimmed.substring(0, eq).trim()
        var value = trimmed.substring(eq + 1).trim()
        if (
            (value.startsWith('"') && value.endsWith('"')) ||
            (value.startsWith('\'') && value.endsWith('\''))
        ) {
            value = value.substring(1, value.length - 1)
        }
        out[key] = value
    }
    return out
}

val monorepoEnvFile = layout.projectDirectory.file("../../../.env").asFile
val monorepoDotEnv: Map<String, String> =
    if (monorepoEnvFile.isFile) parseDotEnv(monorepoEnvFile.readText()) else emptyMap()

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.serialization")
}

android {
    namespace = "com.notepad.kt"
    compileSdk = 36

    defaultConfig {
        minSdk = 24
        consumerProguardFiles("consumer-rules.pro")

        ndk {
            abiFilters += listOf("arm64-v8a", "x86_64")
        }

        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlin {
        compilerOptions {
            jvmTarget.set(JvmTarget.JVM_17)
        }
    }

    sourceSets {
        getByName("main") {
            jniLibs.srcDir("src/main/jniLibs")
        }
    }

    testOptions {
        unitTests.all {
            val jniDir = layout.projectDirectory.dir("src/main/jniLibs/x86_64").asFile
            if (jniDir.isDirectory) {
                val path = jniDir.absolutePath
                it.jvmArgs("-Djava.library.path=$path")
                it.environment("LD_LIBRARY_PATH", path)
            }
            listOf("OPENAI_API_KEY", "OPENAI_MODEL", "OPENAI_BASE_URL").forEach { key ->
                if (System.getenv(key).isNullOrBlank()) {
                    monorepoDotEnv[key]?.takeIf { it.isNotBlank() }?.let { value ->
                        it.environment(key, value)
                    }
                }
            }
        }
    }
}

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.kotlinx.serialization.json)

    testImplementation(libs.junit)
    testImplementation(libs.kotlinx.coroutines.test)
    testImplementation("com.superglue.kt:superglue-android:0.1.0")
}

fun org.gradle.api.Project.resolveAndroidNdkPath(): String? {
    val fromGradle = findProperty("android.ndkPath") as? String
    if (!fromGradle.isNullOrBlank()) {
        return fromGradle.trim()
    }
    listOf("ANDROID_NDK_HOME", "ANDROID_NDK_ROOT").forEach { key ->
        val v = System.getenv(key)
        if (!v.isNullOrBlank()) {
            return v.trim()
        }
    }
    val localProps = rootProject.file("local.properties")
    if (!localProps.exists()) {
        return null
    }
    val p = Properties()
    localProps.inputStream().use { p.load(it) }
    val sdkDir = p.getProperty("sdk.dir")?.trim() ?: return null
    val ndkBase = file(sdkDir).resolve("ndk")
    if (!ndkBase.isDirectory) {
        return null
    }
    val installed = ndkBase.listFiles()?.filter { it.isDirectory }.orEmpty()
    if (installed.isEmpty()) {
        return null
    }
    return installed.maxWithOrNull(
        compareBy<File>(
            { it.name.split(".").firstOrNull()?.toIntOrNull() ?: 0 },
            { it.name },
        ),
    )?.absolutePath
}

val nativeDir = project.layout.projectDirectory.dir("../native").asFile
val jniOutDir = layout.projectDirectory.dir("src/main/jniLibs").asFile
val abis = listOf("arm64-v8a", "x86_64")
val superglueDir = nativeDir.parentFile.parentFile.resolve("superglue")

val resolvedNdkPath: String? = project.resolveAndroidNdkPath()
val canBuildRustJni: Boolean = resolvedNdkPath != null && nativeDir.resolve("Cargo.toml").exists()

val skipNativeBuild =
    (findProperty("notepad.skipNativeBuild") as? String)?.equals("true", ignoreCase = true) == true ||
        System.getenv("NOTEPAD_SKIP_NATIVE_BUILD") == "1"
val shouldWireRustJni: Boolean = canBuildRustJni && !skipNativeBuild
for (abi in abis) {
    File(jniOutDir, abi).mkdirs()
}

val buildRustJni =
    tasks.register<Exec>("buildRustJni") {
        group = "build"
        description = "Build libnotepad_kt.so for Android ABIs via cargo-ndk."
        onlyIf { shouldWireRustJni }

        inputs.file(nativeDir.resolve("Cargo.toml"))
        inputs.dir(nativeDir.resolve("src"))
        for (abi in abis) {
            outputs.file(File(jniOutDir, "$abi/libnotepad_kt.so"))
        }

        doFirst {
            val ndk = resolvedNdkPath
                ?: throw GradleException("NDK not resolved; set ANDROID_NDK_HOME or sdk.dir+ndk/ in local.properties")
            environment("ANDROID_NDK_HOME", ndk)
            environment("ANDROID_NDK_ROOT", ndk)
        }

        workingDir = nativeDir
        commandLine = buildList {
            add("cargo")
            add("ndk")
            add("-o")
            add(jniOutDir.absolutePath)
            abis.forEach { add("-t"); add(it) }
            add("build")
            add("--release")
        }
    }

val workspaceTargetDir = nativeDir.parentFile.parentFile.resolve("target")
val superglueNativeDir = layout.projectDirectory.dir("../../../superglue/superglue-kt/native").asFile
val superglueTargetDir = layout.projectDirectory.dir("../../../superglue/target").asFile

val buildSuperglueJniHost =
    tasks.register<Exec>("buildSuperglueJniHost") {
        group = "build"
        description = "Build libsuperglue_kt.so for host notepad integration tests."
        onlyIf { !skipNativeBuild && superglueNativeDir.resolve("Cargo.toml").exists() }

        inputs.file(superglueNativeDir.resolve("Cargo.toml"))
        inputs.dir(superglueNativeDir.resolve("src"))
        outputs.file(File(jniOutDir, "x86_64/libsuperglue_kt.so"))

        workingDir = superglueNativeDir
        commandLine("cargo", "build", "--release")

        doLast {
            val built = superglueTargetDir.resolve("release/libsuperglue_kt.so")
            if (!built.isFile) {
                throw GradleException("Expected ${built.absolutePath} after cargo build")
            }
            built.copyTo(File(jniOutDir, "x86_64/libsuperglue_kt.so"), overwrite = true)
        }
    }

val buildRustJniHost =
    tasks.register<Exec>("buildRustJniHost") {
        group = "build"
        description = "Build libnotepad_kt.so for host unit tests when NDK is unavailable."
        onlyIf { !shouldWireRustJni && !skipNativeBuild && nativeDir.resolve("Cargo.toml").exists() }

        inputs.file(nativeDir.resolve("Cargo.toml"))
        inputs.dir(nativeDir.resolve("src"))
        outputs.file(File(jniOutDir, "x86_64/libnotepad_kt.so"))

        workingDir = nativeDir
        commandLine("cargo", "build", "--release")

        doLast {
            val built = workspaceTargetDir.resolve("release/libnotepad_kt.so")
            if (!built.isFile) {
                throw GradleException("Expected ${built.absolutePath} after cargo build")
            }
            built.copyTo(File(jniOutDir, "x86_64/libnotepad_kt.so"), overwrite = true)
        }
    }

afterEvaluate {
    val testTask = tasks.findByName("testDebugUnitTest")
    if (shouldWireRustJni) {
        listOf("preBuild", "mergeDebugJniLibFolders", "mergeReleaseJniLibFolders", "testDebugUnitTest")
            .forEach { taskName ->
                tasks.findByName(taskName)?.dependsOn(buildRustJni)
            }
    } else if (!skipNativeBuild) {
        testTask?.dependsOn(buildRustJniHost, buildSuperglueJniHost)
    }
}
