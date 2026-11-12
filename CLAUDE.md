# Nera

Do not include AI or LLM co-author references, AI `Co-Authored-By` trailers, or generated-by attribution in commits or pull request descriptions (for example, “Generated with …” lines).

## JetBrains Plugin

When the JetBrains plugin (`editors/jetbrains-nera/`) is changed, it needs to be rebuilt before testing. Build with Gradle from the plugin directory:

```sh
cd editors/jetbrains-nera && ./gradlew buildPlugin
```
