# JNI looks these up by name; keep them if minification is enabled.
-keep class com.orbit.sdk.bridge.OrbitJni { native <methods>; }
-keep class com.orbit.sdk.bridge.OrbitNativeException { <init>(int, java.lang.String); }
-keep class com.orbit.sdk.platform.SecureStorageException { <init>(java.lang.String); }
