rootProject.name = "oracle"

// A trailing comment, a commented-out include, and a multiline call: the three
// shapes a line scanner gets wrong.
include("core")
// include("never-built")
include(
    "app",
)
