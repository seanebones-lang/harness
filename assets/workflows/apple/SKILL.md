---
name: apple
description: Build or maintain Swift, Xcode, and Apple application projects with explicit build and release evidence.
---

# Apple workflow

Inspect the actual Xcode project or workspace, available schemes, destination, and signing settings. Use xcodebuild -list to select an existing scheme; do not guess it. Run an appropriate simulator build or test before claiming the code works. Inspect changed permission purpose strings, entitlements, assets, and app behavior where relevant. Separate simulator validation, device/signing validation, archive, upload, and App Store publication. Do not claim approval, installs, revenue, or adoption from a successful build or upload. Give an exact repeatable command and identify unavailable Xcode, simulator, signing, or account gates.
