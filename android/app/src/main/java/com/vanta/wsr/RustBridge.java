package com.vanta.wsr;

public final class RustBridge {
    static {
        System.loadLibrary("wsr_jni");
    }

    public static native int initVfs();
    public static native int startServer(int port);
}
