// swift-tools-version:5.9
import PackageDescription

// Build with ./build.sh (it first builds the Rust static library into ./lib).
let package = Package(
    name: "Simplenotorious",
    platforms: [.macOS(.v14)],
    targets: [
        .systemLibrary(name: "CSimplenotorious", path: "Sources/CSimplenotorious"),
        .executableTarget(
            name: "Simplenotorious",
            dependencies: ["CSimplenotorious"],
            path: "Sources/Simplenotorious",
            linkerSettings: [
                .unsafeFlags(["-L", "lib"]),
                .linkedLibrary("simplenotorious_ffi"),
                .linkedLibrary("resolv"),
                .linkedLibrary("iconv"),
                .linkedFramework("Security"),
                .linkedFramework("SystemConfiguration"),
                .linkedFramework("CoreFoundation"),
            ]
        ),
    ]
)
