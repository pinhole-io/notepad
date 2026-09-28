pluginManagement {
    repositories {
        google {
            content {
                includeGroupByRegex("com\\.android.*")
                includeGroupByRegex("com\\.google.*")
                includeGroupByRegex("androidx.*")
            }
        }
        mavenCentral()
        gradlePluginPortal()
    }
}
dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "notepad-kt"
include(":notepad-android")

includeBuild("../../superglue/superglue-kt") {
    dependencySubstitution {
        substitute(module("com.superglue.kt:superglue-android")).using(project(":superglue-android"))
    }
}
