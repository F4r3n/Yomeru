# syntax=docker/dockerfile:1.7
#
# Android toolchain image for yomeru-android: Rust + aarch64 target,
# JDK, Android SDK + NDK, and dioxus-cli. Lets you build the APK on a
# host that only has Podman, without installing the Android SDK.
#
# Unlike app/web/Dockerfile this is a *toolchain* image, not a one-shot
# build: the repo is bind-mounted at run time so edits don't invalidate
# the image, and cargo/gradle caches live in named volumes.
#
# Build the image (once, or after bumping a version ARG):
#
#   podman build -f app/android/Dockerfile -t yomeru-android-build app/android
#
# Build an APK (output under target/podman/dx/yomeru-android/):
#
#   app/android/podman-build.sh                 # debug
#   app/android/podman-build.sh --release       # release
#
# Install it with adb on the host (`pacman -S android-tools`), or run
# `adb` inside the container against a device paired over Wi-Fi.

FROM rust:1-slim-trixie

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        pkg-config build-essential libssl-dev ca-certificates \
        curl unzip git openjdk-21-jdk-headless \
 && rm -rf /var/lib/apt/lists/*

RUN rustup target add aarch64-linux-android x86_64-linux-android

# dx refuses to bundle against a different dioxus version: keep in step
# with the dioxus version in Cargo.lock. On a bump, also re-check our copies
# of dx's Android templates (see "Upgrading dx" in app/README.md).
ARG DX_VERSION=0.7.10
RUN curl -sSL https://dioxus.dev/install.sh | bash -s "v${DX_VERSION}"

# ---------- Android SDK + NDK ----------
ARG CMDLINE_TOOLS=11076708
ARG ANDROID_PLATFORM=35
ARG BUILD_TOOLS=35.0.0
ARG NDK_VERSION=27.2.12479018

ENV ANDROID_HOME=/opt/android-sdk
ENV ANDROID_SDK_ROOT=${ANDROID_HOME}
ENV ANDROID_NDK_HOME=${ANDROID_HOME}/ndk/${NDK_VERSION}
ENV NDK_HOME=${ANDROID_NDK_HOME}
ENV JAVA_HOME=/usr/lib/jvm/java-21-openjdk-amd64
ENV PATH="/usr/local/cargo/bin:/root/.cargo/bin:${ANDROID_HOME}/cmdline-tools/latest/bin:${ANDROID_HOME}/platform-tools:${PATH}"

RUN mkdir -p ${ANDROID_HOME}/cmdline-tools \
 && curl -sSL -o /tmp/clt.zip \
        "https://dl.google.com/android/repository/commandlinetools-linux-${CMDLINE_TOOLS}_latest.zip" \
 && unzip -q /tmp/clt.zip -d /tmp/clt \
 && mv /tmp/clt/cmdline-tools ${ANDROID_HOME}/cmdline-tools/latest \
 && rm -rf /tmp/clt /tmp/clt.zip \
 && yes | sdkmanager --licenses > /dev/null \
 && sdkmanager --install \
        "platform-tools" \
        "platforms;android-${ANDROID_PLATFORM}" \
        "build-tools;${BUILD_TOOLS}" \
        "ndk;${NDK_VERSION}" > /dev/null

# bundletool turns the release .aab (Gradle's release build type: not
# debuggable, minified) into a universal APK, signed with the release key.
ARG BUNDLETOOL_VERSION=1.17.2
RUN curl -sSL -o /opt/bundletool.jar \
        "https://github.com/google/bundletool/releases/download/${BUNDLETOOL_VERSION}/bundletool-all-${BUNDLETOOL_VERSION}.jar"
ENV BUNDLETOOL=/opt/bundletool.jar

# Separate from the host's target/ so host and container builds (different
# rustc/glibc) don't keep invalidating each other's artifacts.
ENV CARGO_TARGET_DIR=/src/target/podman

WORKDIR /src/app/android
CMD ["dx", "bundle", "--package", "yomeru-android", "--platform", "android"]
