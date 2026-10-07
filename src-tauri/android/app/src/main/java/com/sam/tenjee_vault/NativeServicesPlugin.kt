package com.sam.tenjee_vault

import android.app.Activity
import android.content.ClipData
import android.content.Intent
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.provider.OpenableColumns
import android.webkit.MimeTypeMap
import android.webkit.WebView
import androidx.activity.result.ActivityResult
import androidx.core.content.FileProvider
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import app.tauri.plugin.Channel
import android.net.wifi.WifiManager
import java.io.File
import java.util.UUID
import java.util.concurrent.Executors

@InvokeArg
class PickArgs { var multiple = false; var extensions: Array<String> = emptyArray() }
@InvokeArg
class ExportArgs { lateinit var path: String; var share = false }
@InvokeArg
class ScheduleArgs { var reminders: Array<NativeReminder> = emptyArray(); var horizon = 0L }
@InvokeArg
class AckArgs { var keys: Array<String> = emptyArray() }
@InvokeArg
class BackupPublishArgs { var retention = 10 }
@InvokeArg
class ExchangeArgs { lateinit var suspended: Channel; lateinit var token: String; var discovery = true }
@InvokeArg
class EndExchangeArgs { var token = "" }

@InvokeArg
class TransferArgs {
  var folder = false; var allowStaging = false; var source = ""; var destination = ""
  var uri = ""; var token = ""; var batch = ""; var name = ""; var id = 0
  var path: Array<String> = emptyArray(); var size = "0"
}

@TauriPlugin
class NativeServicesPlugin(private val activity: Activity) : Plugin(activity) {
  private val worker = Executors.newSingleThreadExecutor()
  private val transferDocuments = TransferDocuments(activity)
  private var exportSource: File? = null
  private var multicast: WifiManager.MulticastLock? = null
  private var suspendedChannel: Channel? = null
  private var exchangeToken = ""
  private val transferRoot get() = File(activity.cacheDir, "tenjee-transfer").apply { mkdirs() }

  override fun load(webView: WebView) {
    val old = transferRoot.listFiles()?.filter { it.lastModified() < System.currentTimeMillis() - 10 * 60 * 1000L } ?: emptyList()
    worker.execute { old.forEach { it.deleteRecursively() } }
  }

  @Suppress("OVERRIDE_DEPRECATION")
  override fun onPause() {
    transferDocuments.cancel()
    activity.window.clearFlags(android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
    suspendedChannel?.send(JSObject().put("suspended", true))
    suspendedChannel = null
    multicast?.let { if (it.isHeld) it.release() }
    multicast = null
  }

  @Command
  fun beginExchange(invoke: Invoke) {
    try {
      multicast?.let { if (it.isHeld) it.release() }
      require(activity.hasWindowFocus() && !activity.isFinishing) { "Keep the app in the foreground to exchange" }
      val args = invoke.parseArgs(ExchangeArgs::class.java)
      suspendedChannel = args.suspended
      exchangeToken = args.token
      transferDocuments.activityProgress = { suspendedChannel?.send(JSObject().put("activity", true)) }
      transferDocuments.begin()
      activity.window.addFlags(android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
      if (args.discovery) {
        val wifi = activity.applicationContext.getSystemService(android.content.Context.WIFI_SERVICE) as WifiManager
        multicast = wifi.createMulticastLock("tenjee-exchange").apply { setReferenceCounted(false); acquire() }
      }
      invoke.resolve()
    } catch (_: Exception) { invoke.reject("Local network discovery permission unavailable") }
  }

  @Command
  fun endExchange(invoke: Invoke) {
    if (invoke.parseArgs(EndExchangeArgs::class.java).token != exchangeToken) { invoke.resolve(); return }
    suspendedChannel = null
    transferDocuments.activityProgress = null
    transferDocuments.cancel()
    activity.window.clearFlags(android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
    multicast?.let { if (it.isHeld) it.release() }
    multicast = null
    invoke.resolve()
  }


  @Command
  fun pickTransferSources(invoke: Invoke) {
    val args = invoke.parseArgs(TransferArgs::class.java)
    val intent = if (args.folder) Intent(Intent.ACTION_OPEN_DOCUMENT_TREE) else Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
      addCategory(Intent.CATEGORY_OPENABLE); type = "*/*"; putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true)
    }
    intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION)
    startActivityForResult(invoke, intent, "transferSourcesPicked")
  }
  @ActivityCallback
  fun transferSourcesPicked(invoke: Invoke, result: ActivityResult) {
    if (result.resultCode != Activity.RESULT_OK) { invoke.resolve(JSObject().put("sources", JSArray())); return }
    val data = result.data ?: run { invoke.reject("Picker returned no documents"); return }
    val uris = mutableListOf<Uri>(); data.clipData?.let { clip -> for (i in 0 until clip.itemCount) uris.add(clip.getItemAt(i).uri) }
    if (uris.isEmpty()) data.data?.let { uris.add(it) }
    worker.execute { try {
      require(uris.size <= 1000) { "Select at most 1,000 roots" }
      val rows = JSArray(); uris.distinct().forEach { rows.put(transferDocuments.remember(it, data.flags)) }
      invoke.resolve(JSObject().put("sources", rows))
    } catch (error: Exception) { invoke.reject(error.message ?: "Cannot access selected documents") } }
  }
  @Command
  fun pickTransferDestination(invoke: Invoke) {
    val intent = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION or Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION)
    startActivityForResult(invoke, intent, "transferDestinationPicked")
  }
  @ActivityCallback
  fun transferDestinationPicked(invoke: Invoke, result: ActivityResult) {
    val uri = result.data?.data
    if (result.resultCode != Activity.RESULT_OK || uri == null) { invoke.resolve(JSObject().put("uri", null)); return }
    worker.execute { try {
      val source = transferDocuments.remember(uri, result.data!!.flags); transferDocuments.identity(uri.toString())
      invoke.resolve(JSObject().put("uri", uri.toString()).put("name", source.getString("name")))
    } catch (error: Exception) { invoke.reject(error.message ?: "Choose a writable document folder") } }
  }
  private fun transferWork(invoke: Invoke, operation: (TransferArgs) -> JSObject) {
    val args = invoke.parseArgs(TransferArgs::class.java)
    worker.execute { try { invoke.resolve(operation(args)) } catch (error: Exception) { invoke.reject(error.message ?: "Native file operation failed") } }
  }
  @Command fun transferDestinationIdentity(invoke: Invoke) = transferWork(invoke) { transferDocuments.identity(it.uri) }
  @Command fun beginTransferScan(invoke: Invoke) = transferWork(invoke) { transferDocuments.startScan(it.source, it.batch, it.allowStaging) }
  @Command fun nextTransferScan(invoke: Invoke) = transferWork(invoke) { transferDocuments.nextScan(it.token) }
  @Command fun cancelTransferScan(invoke: Invoke) = transferWork(invoke) { transferDocuments.cancelScan(it.token); JSObject() }
  @Command fun cancelTransferPreparation(invoke: Invoke) { transferDocuments.cancel(); invoke.resolve() }
  @Command fun openTransferSource(invoke: Invoke) = transferWork(invoke) { transferDocuments.openSource(it.source) }
  @Command fun createTransferBatch(invoke: Invoke) = transferWork(invoke) { transferDocuments.createBatch(it.destination, it.name, it.batch) }
  @Command fun restoreTransferBatch(invoke: Invoke) = transferWork(invoke) { transferDocuments.restoreBatch(it.uri) }
  @Command fun verifyTransferDirectory(invoke: Invoke) = transferWork(invoke) { transferDocuments.verifyDirectory(it.uri, it.path.toList()); JSObject() }
  @Command fun createTransferDirectory(invoke: Invoke) = transferWork(invoke) { transferDocuments.createDirectory(it.uri, it.path.toList()); JSObject() }
  @Command fun publishTransferEntry(invoke: Invoke) = transferWork(invoke) { transferDocuments.publish(it.uri, it.id, it.path.toList(), it.source, it.size.toLong()); JSObject() }
  @Command fun readSavedTransferEntry(invoke: Invoke) = transferWork(invoke) { transferDocuments.readSaved(it.uri, it.id, it.path.toList(), it.size.toLong()) }
  @Command fun discardTransferBatch(invoke: Invoke) = transferWork(invoke) { transferDocuments.discard(it.batch); JSObject() }
  @Command fun openTransferDestination(invoke: Invoke) {
    val uri = Uri.parse(invoke.parseArgs(TransferArgs::class.java).uri)
    try { activity.startActivity(Intent(Intent.ACTION_VIEW).setDataAndType(uri, android.provider.DocumentsContract.Document.MIME_TYPE_DIR).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)); invoke.resolve() }
    catch (_: Exception) { invoke.reject("Open your system Files app to view the receive folder") }
  }

  @Command
  fun pickFiles(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(PickArgs::class.java)
      val mapped = args.extensions.map { MimeTypeMap.getSingleton().getMimeTypeFromExtension(it) }
      val types = mapped.filterNotNull().distinct()
      val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
        addCategory(Intent.CATEGORY_OPENABLE)
        type = "*/*"
        if (types.isNotEmpty() && mapped.all { it != null }) putExtra(Intent.EXTRA_MIME_TYPES, types.toTypedArray())
        putExtra(Intent.EXTRA_ALLOW_MULTIPLE, args.multiple)
      }
      startActivityForResult(invoke, intent, "pickedFiles")
    } catch (_: Exception) { invoke.reject("Cannot open document picker") }
  }

  @ActivityCallback
  fun pickedFiles(invoke: Invoke, result: ActivityResult) {
    if (result.resultCode != Activity.RESULT_OK) {
      invoke.resolve(JSObject().put("paths", JSArray()))
      return
    }
    val uris = mutableListOf<Uri>()
    result.data?.clipData?.let { clip -> for (i in 0 until clip.itemCount) uris.add(clip.getItemAt(i).uri) }
    if (uris.isEmpty()) result.data?.data?.let { uris.add(it) }
    if (uris.size > 100) { invoke.reject("Select at most 100 documents"); return }
    worker.execute {
      val directory = File(transferRoot, UUID.randomUUID().toString()).apply { mkdir() }
      try {
        var batchBytes = 0L
        val paths = uris.mapIndexed { index, uri ->
          var name = "document-$index"
          activity.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { cursor ->
            if (cursor.moveToFirst()) name = cursor.getString(0) ?: name
          }
          name = name.substringAfterLast('/').substringAfterLast('\\').replace(Regex("[\\x00-\\x1f]"), "_").take(180)
          if (name == "." || name == ".." || name.isEmpty()) name = "document-$index"
          val itemDirectory = File(directory, "$index").apply { mkdir() }
          val file = File(itemDirectory, name)
          activity.contentResolver.openInputStream(uri)?.use { input ->
            file.outputStream().use { output ->
              val buffer = ByteArray(65536); var total = 0L
              while (true) {
                val n = input.read(buffer); if (n < 0) break
                total += n
                batchBytes += n
                if (total > 1024L * 1024 * 1024 || batchBytes > 1024L * 1024 * 1024) throw IllegalArgumentException("Documents exceed 1 GiB")
                output.write(buffer, 0, n)
              }
            }
          } ?: throw IllegalArgumentException("Document access was denied")
          file.absolutePath
        }
        invoke.resolve(JSObject().put("paths", JSArray.from(paths.toTypedArray())))
      } catch (_: Exception) {
        directory.deleteRecursively()
        invoke.reject("Cannot read selected document; access denied or size limit exceeded")
      }
    }
  }

  private fun checkedFile(path: String): File {
    val file = File(path).canonicalFile
    require(file.isFile && file.toPath().startsWith(transferRoot.canonicalFile.toPath()))
    return file
  }

  @Command
  fun exportFile(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(ExportArgs::class.java)
      val file = checkedFile(args.path)
      val mime = MimeTypeMap.getSingleton().getMimeTypeFromExtension(file.extension.lowercase()) ?: "application/octet-stream"
      if (args.share) {
        val uri = FileProvider.getUriForFile(activity, "${activity.packageName}.fileprovider", file)
        val intent = Intent(Intent.ACTION_SEND).apply {
          type = mime; putExtra(Intent.EXTRA_STREAM, uri)
          clipData = ClipData.newRawUri(file.name, uri)
          addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        activity.startActivity(Intent.createChooser(intent, null))
        Handler(Looper.getMainLooper()).postDelayed({ file.delete() }, 10 * 60 * 1000L)
        invoke.resolve(JSObject().put("completed", true))
      } else {
        require(exportSource == null) { "Another save is in progress" }
        exportSource = file
        val intent = Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
          addCategory(Intent.CATEGORY_OPENABLE); type = mime
          putExtra(Intent.EXTRA_TITLE, file.name)
        }
        startActivityForResult(invoke, intent, "savedFile")
      }
    } catch (_: Exception) { exportSource = null; invoke.reject("Cannot open save/share dialog") }
  }

  @ActivityCallback
  fun savedFile(invoke: Invoke, result: ActivityResult) {
    val file = exportSource; exportSource = null
    if (result.resultCode != Activity.RESULT_OK || result.data?.data == null || file == null) {
      file?.delete(); invoke.resolve(JSObject().put("completed", false)); return
    }
    val uri = result.data!!.data!!
    worker.execute {
      try {
        activity.contentResolver.openOutputStream(uri, "wt")?.use { output -> file.inputStream().use { it.copyTo(output, 65536) } }
          ?: throw IllegalArgumentException("Destination access denied")
        invoke.resolve(JSObject().put("completed", true))
      } catch (_: Exception) {
        // A provider can fail partway through writing. Remove the newly created
        // document when supported rather than report a partial file as success.
        try { android.provider.DocumentsContract.deleteDocument(activity.contentResolver, uri) } catch (_: Exception) { }
        invoke.reject("Cannot write document; destination access denied or storage full")
      } finally { file.delete() }
    }
  }

  @Command
  fun scheduleReminders(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(ScheduleArgs::class.java)
      invoke.resolve(ReminderAlarms.replace(activity, args.reminders, args.horizon))
    } catch (_: Exception) { invoke.reject("Cannot schedule Android reminders") }
  }

  @Command
  fun reminderStatus(invoke: Invoke) { invoke.resolve(ReminderAlarms.status(activity)) }

  @Command
  fun drainReminderFires(invoke: Invoke) { invoke.resolve(ReminderAlarms.drainFires(activity)) }

  @Command
  fun ackReminderFires(invoke: Invoke) {
    ReminderAlarms.ackFires(activity, invoke.parseArgs(AckArgs::class.java).keys)
    invoke.resolve()
  }

  @Command
  fun openAlarmSettings(invoke: Invoke) {
    if (android.os.Build.VERSION.SDK_INT >= 31) {
      activity.startActivity(Intent(android.provider.Settings.ACTION_REQUEST_SCHEDULE_EXACT_ALARM, Uri.parse("package:${activity.packageName}")))
    }
    invoke.resolve()
  }

  @Command
  fun chooseBackupDirectory(invoke: Invoke) {
    startActivityForResult(invoke, Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION or Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION), "backupDirectoryPicked")
  }

  @ActivityCallback
  fun backupDirectoryPicked(invoke: Invoke, result: ActivityResult) {
    val uri = result.data?.data
    if (result.resultCode != Activity.RESULT_OK || uri == null) { invoke.resolve(JSObject().put("directory", null)); return }
    worker.execute {
    try {
      val flags = result.data!!.flags and (Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
      activity.contentResolver.takePersistableUriPermission(uri, flags)
      val previous = activity.getSharedPreferences("backup-documents", Activity.MODE_PRIVATE).getString("tree", null)
      val editor = activity.getSharedPreferences("backup-documents", Activity.MODE_PRIVATE).edit().putString("tree", uri.toString())
      if (previous != uri.toString()) {
        val interrupted = activity.getSharedPreferences("backup-documents", Activity.MODE_PRIVATE).getString("pendingDocument", null)
        if (interrupted != null) {
          android.provider.DocumentsContract.deleteDocument(activity.contentResolver, Uri.parse(interrupted))
          editor.remove("pendingDocument")
        }
        editor.putString("published", "{}")
      }
      check(editor.commit())
      val directory = File(activity.filesDir, "tenjee-automatic-backups").apply { mkdirs() }
      invoke.resolve(JSObject().put("directory", directory.absolutePath))
    } catch (_: Exception) { invoke.reject("Backup folder access was denied") }
    }
  }

  @Command
  fun publishBackups(invoke: Invoke) {
    val args = invoke.parseArgs(BackupPublishArgs::class.java)
    worker.execute {
      try {
        val store = activity.getSharedPreferences("backup-documents", Activity.MODE_PRIVATE)
        val selected = store.getString("tree", null)
        if (selected == null) { invoke.resolve(); return@execute }
        val tree = Uri.parse(selected)
        val parent = android.provider.DocumentsContract.buildDocumentUriUsingTree(tree, android.provider.DocumentsContract.getTreeDocumentId(tree))
        val published = org.json.JSONObject(store.getString("published", "{}") ?: "{}")
        val interrupted = store.getString("pendingDocument", null)
        if (interrupted != null) {
          try { android.provider.DocumentsContract.deleteDocument(activity.contentResolver, Uri.parse(interrupted)) } catch (_: Exception) { throw IllegalArgumentException("Cannot clean up interrupted backup copy") }
          check(store.edit().remove("pendingDocument").commit())
        }
        val directory = File(activity.filesDir, "tenjee-automatic-backups")
        for (file in (directory.listFiles() ?: emptyArray()).filter { it.isFile && it.name.startsWith("tenjee-vault-auto-") && it.extension == "tvault" }.sortedBy { it.name }) {
          if (published.has(file.name)) continue
          val uri = android.provider.DocumentsContract.createDocument(activity.contentResolver, parent, "application/octet-stream", file.name)
            ?: throw IllegalArgumentException("Cannot create backup document")
          check(store.edit().putString("pendingDocument", uri.toString()).commit())
          try {
            activity.contentResolver.openOutputStream(uri, "wt")?.use { output -> file.inputStream().use { it.copyTo(output, 65536) } }
              ?: throw IllegalArgumentException("Backup access denied")
            published.put(file.name, uri.toString())
            check(store.edit().putString("published", published.toString()).remove("pendingDocument").commit())
          } catch (error: Exception) { try { android.provider.DocumentsContract.deleteDocument(activity.contentResolver, uri) } catch (_: Exception) { }; throw error }
        }
        val names = published.keys().asSequence().toList().sorted()
        for (name in names.take((names.size - args.retention.coerceIn(1, 365)).coerceAtLeast(0))) {
          if (android.provider.DocumentsContract.deleteDocument(activity.contentResolver, Uri.parse(published.getString(name)))) published.remove(name)
        }
        check(store.edit().putString("published", published.toString()).commit())
        invoke.resolve()
      } catch (_: Exception) { invoke.reject("Backup copy pending: check the selected folder permission and available storage") }
    }
  }
}
