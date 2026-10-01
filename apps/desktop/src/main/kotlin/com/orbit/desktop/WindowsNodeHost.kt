package com.orbit.desktop

import com.orbit.client.features.host.HostCard
import com.orbit.client.features.host.HostedNode
import com.orbit.client.features.host.NodeHost
import com.orbit.client.features.host.NodeHostException
import java.io.File
import java.util.concurrent.CompletableFuture
import java.util.concurrent.ExecutionException
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext

/**
 * Runs the bundled `orbit-node` program and keeps it alive until [stop].
 * A second call while that process is up returns the same address and code.
 */
class WindowsNodeHost(private val executable: File?) : NodeHost {
    private val mutex = Mutex()
    private var process: Process? = null
    private var card: HostedNode? = null

    init {
        Runtime.getRuntime().addShutdownHook(Thread { process?.destroy() })
    }

    override suspend fun becomeNode(): HostedNode = mutex.withLock {
        val current = process
        val known = card
        if (current != null && current.isAlive && known != null) return@withLock known
        withContext(Dispatchers.IO) { start() }
    }

    fun stop() {
        process?.destroy()
        process = null
    }

    private fun start(): HostedNode {
        val program = executable
        if (program == null || !program.isFile) {
            throw NodeHostException(
                "Не найден OrbitNode.exe. Соберите узел и положите orbit-node.exe рядом с клиентом.",
            )
        }
        val started = ProcessBuilder(program.absolutePath)
            .redirectErrorStream(true)
            .start()
        process = started
        val output = StringBuilder()
        val found = CompletableFuture<HostedNode>()
        Thread({
            started.inputStream.bufferedReader(Charsets.UTF_8).use { reader ->
                while (true) {
                    val line = reader.readLine() ?: break
                    output.append(line).append('\n')
                    val parsed = HostCard.parse(output.toString())
                    if (parsed != null) found.complete(parsed)
                }
            }
            if (!found.isDone) found.completeExceptionally(NodeHostException(failureText(output)))
        }, "orbit-node").apply { isDaemon = true }.start()
        val result = try {
            found.get(20, TimeUnit.SECONDS)
        } catch (timeout: TimeoutException) {
            started.destroy()
            process = null
            throw NodeHostException("Узел не сообщил адрес за 20 секунд.")
        } catch (failure: ExecutionException) {
            started.destroy()
            process = null
            throw (failure.cause as? NodeHostException) ?: NodeHostException(failure.cause?.message ?: "Узел не запустился.")
        }
        card = result
        return result
    }
}

internal fun locateOrbitNodeExecutable(): File? {
    System.getProperty("orbit.node.executable")?.let { configured ->
        return File(configured).takeIf(File::isFile)
    }
    val names = listOf("orbit-node.exe", "OrbitNode.exe")
    val roots = mutableListOf<File>()
    System.getProperty("compose.application.resources.dir")?.let { roots += File(it) }
    var directory: File? = File(System.getProperty("user.dir"))
    repeat(6) {
        val current = directory ?: return@repeat
        roots += current
        roots += File(current, "dist")
        roots += File(current, "target/release")
        directory = current.parentFile
    }
    return roots.asSequence().flatMap { root -> names.asSequence().map { name -> File(root, name) } }.firstOrNull(File::isFile)
}

private fun failureText(output: StringBuilder): String {
    val tail = output.lineSequence().map(String::trim).filter(String::isNotEmpty).toList().takeLast(8)
    val detail = tail.joinToString("\n").ifBlank { "процесс узла завершился, не напечатав адрес" }
    return "Не удалось запустить узел.\n$detail"
}
