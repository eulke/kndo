rootProject.name = "demo"

include("core")

// include("legacy") — a comment names no module, and Gradle builds nothing here.

include(
    "app",
)
