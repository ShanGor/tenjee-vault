package com.sam.tenjee_vault

import android.app.Activity
import android.content.Intent
import android.database.Cursor
import android.database.sqlite.SQLiteDatabase
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.provider.DocumentsContract as DC
import android.system.Os
import android.system.OsConstants
import app.tauri.plugin.JSObject
import org.json.JSONArray
import java.io.Closeable
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream
import java.security.MessageDigest
import java.text.Normalizer
import java.util.UUID
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference

/** File bodies stay in native streams/descriptors; only bounded metadata enters IPC. */
class TransferDocuments(private val activity: Activity) {
  private val resolver get() = activity.contentResolver
  private val privateRoot get() = File(activity.filesDir, "file-exchange-native").apply { mkdirs() }
  var activityProgress: (() -> Unit)? = null
  private var lastActivity = 0L
  private val cancelled = AtomicBoolean(false)
  private val input = AtomicReference<Closeable?>()
  private val output = AtomicReference<Closeable?>()
  private val scans = mutableMapOf<String, Scan>()
  private val roots = activity.getSharedPreferences("file-transfer-grants", Activity.MODE_PRIVATE)
  private val database: SQLiteDatabase by lazy {
    SQLiteDatabase.openOrCreateDatabase(File(privateRoot, "providers.sqlite"), null).apply {
      execSQL("PRAGMA synchronous=FULL")
      execSQL("CREATE TABLE IF NOT EXISTS batches(batch TEXT PRIMARY KEY, uri TEXT UNIQUE, parent TEXT, private_path TEXT)")
      execSQL("CREATE TABLE IF NOT EXISTS entries(batch TEXT,id INTEGER,path TEXT,saved TEXT,temp TEXT,state TEXT,PRIMARY KEY(batch,id))")
      execSQL("CREATE TABLE IF NOT EXISTS directories(batch TEXT,path TEXT,uri TEXT,PRIMARY KEY(batch,path))")
      execSQL("CREATE TABLE IF NOT EXISTS grants(batch TEXT,uri TEXT,PRIMARY KEY(batch,uri))")
      execSQL("CREATE TABLE IF NOT EXISTS source_stage(batch TEXT,key TEXT,path TEXT,size TEXT,PRIMARY KEY(batch,key))")
    }
  }
  private val projection = arrayOf(DC.Document.COLUMN_DOCUMENT_ID, DC.Document.COLUMN_DISPLAY_NAME,
    DC.Document.COLUMN_MIME_TYPE, DC.Document.COLUMN_SIZE, DC.Document.COLUMN_FLAGS)
  private data class Document(val uri: Uri, val name: String, val mime: String, val size: Long?, val flags: Int)
  private data class Level(val uri: Uri, val path: List<String>, val cursor: Cursor)
  private data class Scan(val root: Uri, val batch: String, val staging: Boolean,
    var first: Document?, val levels: java.util.ArrayDeque<Level> = java.util.ArrayDeque())

  fun begin() { cancelled.set(false) }
  fun cancel() { cancelled.set(true); try { input.getAndSet(null)?.close() } catch (_: Exception) {}
    try { output.getAndSet(null)?.close() } catch (_: Exception) {} }
  private fun check() { check(!cancelled.get()) { "File operation stopped; pair again to resume" } }
  private fun progressed() { val now = android.os.SystemClock.elapsedRealtime(); if (now - lastActivity >= 250) { lastActivity = now; activityProgress?.invoke() } }
  private fun hex(bytes: ByteArray) = bytes.joinToString("") { "%02x".format(it.toInt() and 255) }
  private fun document(uri: Uri): Uri = if (DC.isTreeUri(uri) && uri.path?.contains("/document/") != true)
    DC.buildDocumentUriUsingTree(uri, DC.getTreeDocumentId(uri)) else uri
  private fun row(cursor: Cursor, uri: Uri): Document = Document(uri, cursor.getString(1) ?: "document",
    cursor.getString(2) ?: "application/octet-stream", if (cursor.isNull(3) || cursor.getLong(3) < 0) null else cursor.getLong(3), cursor.getInt(4)).also { require(it.name.toByteArray(Charsets.UTF_8).size <= 255 && it.uri.toString().length <= 8192) { "Provider metadata exceeds supported limits; choose a local file or folder" } }
  private fun metadata(uri: Uri): Document = resolver.query(document(uri), projection, null, null, null)?.use {
    require(it.moveToFirst()) { "Document no longer exists" }; row(it, document(uri))
  } ?: error("Cannot read selected document")
  private fun children(uri: Uri): Cursor {
    val query = DC.buildChildDocumentsUriUsingTree(uri, DC.getDocumentId(document(uri)))
    return resolver.query(query, projection, null, null, null) ?: error("Cannot enumerate selected folder")
  }
  fun remember(uri: Uri, flags: Int): JSObject {
    val access = flags and (Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
    require(access and Intent.FLAG_GRANT_READ_URI_PERMISSION != 0) { "Read permission was not granted" }
    resolver.takePersistableUriPermission(uri, access)
    check(roots.edit().putInt(uri.toString(), roots.getInt(uri.toString(), 0) or access).commit()) { "Cannot persist file permission" }
    val meta = metadata(uri)
    return JSObject().put("source", uri.toString()).put("name", meta.name).put("folder", meta.mime == DC.Document.MIME_TYPE_DIR)
  }
  fun identity(uri: String): JSObject {
    val parsed = Uri.parse(uri); val meta = metadata(parsed)
    require(meta.mime == DC.Document.MIME_TYPE_DIR && meta.flags and DC.Document.FLAG_DIR_SUPPORTS_CREATE != 0) { "Choose a writable local document folder" }
    val digest = MessageDigest.getInstance("SHA-256").digest((parsed.authority + ":" + DC.getDocumentId(document(parsed))).toByteArray())
    // Positive, exactly representable JSON integers. The complete URI remains
    // independently bound to the batch and its persisted grant.
    fun number(start: Int): Long { var value = 0L; for (i in start until start + 6) value = (value shl 8) or (digest[i].toLong() and 255); return value }
    return JSObject().put("identity", JSObject().put("device", number(0)).put("inode", number(6)))
  }
  fun startScan(source: String, batch: String, staging: Boolean): JSObject {
    UUID.fromString(batch); begin(); val root = Uri.parse(source)
    val token = UUID.randomUUID().toString(); scans[token] = Scan(root, batch, staging, metadata(root))
    database.execSQL("INSERT OR IGNORE INTO grants VALUES(?,?)", arrayOf(batch, source))
    return JSObject().put("token", token)
  }
  private fun child(level: Level, cursor: Cursor): Document = row(cursor,
    DC.buildDocumentUriUsingTree(level.uri, cursor.getString(0)))
  fun nextScan(token: String): JSObject {
    val scan = scans[token] ?: error("Scan expired")
    val rows = JSONArray()
    try {
      while (rows.length() < 100) {
        check()
        var item: Pair<Document, List<String>>? = null
        val first = scan.first
        if (first != null) { scan.first = null; item = first to emptyList() }
        else {
          while (scan.levels.isNotEmpty()) {
            val level = scan.levels.last
            if (level.cursor.moveToNext()) { val doc = child(level, level.cursor); item = doc to (level.path + doc.name); break }
            level.cursor.close(); scan.levels.removeLast()
          }
        }
        if (item == null) break
        val (doc, path) = item
        val selectionKey = doc.uri.authority + ":" + DC.getDocumentId(doc.uri)
        require(path.size < 128) { "Folder depth exceeds 128 components" }
        if (doc.mime == DC.Document.MIME_TYPE_DIR) {
          val cursor = children(doc.uri)
          scan.levels.addLast(Level(doc.uri, path, cursor))
          rows.put(JSObject().put("selectionKey", selectionKey).put("path", JSONArray(path)).put("kind", "directory").put("size", "0").put("source", doc.uri.toString()).put("reason", ""))
        } else {
          try {
            val descriptor = resolver.openFileDescriptor(doc.uri, "r") ?: error("Document access denied")
            var regular = false; var length = 0L; var firstIdentity: Pair<Long, Long>? = null
            descriptor.use { fd ->
              val stat = Os.fstat(fd.fileDescriptor); length = stat.st_size
              regular = OsConstants.S_ISREG(stat.st_mode) && length >= 0; firstIdentity = stat.st_dev to stat.st_ino
              if (regular) try { Os.lseek(fd.fileDescriptor, 0, OsConstants.SEEK_SET) } catch (_: Exception) { regular = false }
            }
            if (regular) resolver.openFileDescriptor(doc.uri, "r")?.use { fd -> val stat = Os.fstat(fd.fileDescriptor); if (firstIdentity != (stat.st_dev to stat.st_ino) || stat.st_size != length) regular = false } ?: run { regular = false }
            if (doc.size != null && doc.size != length) regular = false
            val source: String
            if (!regular || doc.size == null) {
              require(scan.staging) { "Document needs staging; enable staging or choose a local file" }
              val cache = File(privateRoot, "sources/${scan.batch}").apply { mkdirs() }
              val prior = scalar("SELECT path FROM source_stage WHERE batch=? AND key=?", scan.batch, selectionKey)
              val staged = if (prior != null) File(prior) else File(cache, "${UUID.randomUUID()}.source")
              if (prior == null) { copyToPrivate(doc.uri, staged, doc.size); database.execSQL("INSERT INTO source_stage VALUES(?,?,?,?)", arrayOf(scan.batch, selectionKey, staged.absolutePath, staged.length().toString())) }
              require(staged.isFile) { "Staged source unavailable; prepare a new batch" }; source = staged.absolutePath; length = staged.length()
            } else { source = doc.uri.toString() }
            rows.put(JSObject().put("selectionKey", selectionKey).put("path", JSONArray(path)).put("kind", "file").put("size", length.toString()).put("source", source).put("reason", ""))
          } catch (error: Exception) {
            check()
            rows.put(JSObject().put("selectionKey", selectionKey).put("path", JSONArray(path)).put("kind", "excluded").put("size", "0").put("source", "").put("reason", if (error.message in listOf("Document needs staging; enable staging or choose a local file", "Insufficient private staging space", "Provider file length changed")) error.message!! else "Cannot read document; check permission, storage, and provider support"))
          }
        }
      }
      val done = scan.first == null && scan.levels.isEmpty()
      if (done) scans.remove(token)
      return JSObject().put("entries", rows).put("done", done)
    } catch (error: Exception) { cancelScan(token); throw error }
  }
  fun cancelScan(token: String) { scans.remove(token)?.levels?.forEach { it.cursor.close() } }
  fun openSource(source: String): JSObject {
    val descriptor = resolver.openFileDescriptor(Uri.parse(source), "r") ?: error("Document access denied")
    try {
      require(OsConstants.S_ISREG(Os.fstat(descriptor.fileDescriptor).st_mode)) { "Source requires staging" }
      Os.lseek(descriptor.fileDescriptor, 0, OsConstants.SEEK_SET)
      return JSObject().put("fd", descriptor.detachFd())
    } finally { descriptor.close() }
  }
  private fun syncDirectory(file: File) { val descriptor = Os.open(file.absolutePath, OsConstants.O_RDONLY, 0); try { require(OsConstants.S_ISDIR(Os.fstat(descriptor).st_mode)) { "Expected staging directory" }; Os.fsync(descriptor) } finally { Os.close(descriptor) } }
  private fun copyToPrivate(uri: Uri, file: File, size: Long?) {
    val source = resolver.openInputStream(uri) ?: error("Cannot read document"); input.set(source)
    try {
      FileOutputStream(file).use { target -> output.set(target); val bytes = ByteArray(65536); var copied = 0L
        while (true) { check(); val n = source.read(bytes); if (n < 0) break
          require(file.parentFile!!.usableSpace >= 32L * 1024 * 1024 + n) { "Insufficient private staging space" }
          copied = Math.addExact(copied, n.toLong()); require(size == null || copied <= size) { "Provider file length changed" }; target.write(bytes, 0, n); progressed()
        }
        require(size == null || copied == size) { "Provider file length changed" }; target.fd.sync()
      }
      syncDirectory(file.parentFile!!); syncDirectory(file.parentFile!!.parentFile!!)
    } catch (error: Exception) { file.delete(); throw error }
    finally { input.set(null); output.set(null); source.close() }
  }
  private fun scalar(sql: String, vararg values: String): String? = database.rawQuery(sql, values).use { if (it.moveToFirst()) it.getString(0) else null }
  private fun normalized(name: String) = Normalizer.normalize(name, Normalizer.Form.NFC).lowercase(java.util.Locale.ROOT)
  private fun find(parent: Uri, name: String): Document? = children(parent).use { cursor ->
    while (cursor.moveToNext()) if (normalized(cursor.getString(1) ?: "") == normalized(name)) return@use child(Level(parent, emptyList(), cursor), cursor)
    null
  }
  fun createBatch(destination: String, name: String, batch: String): JSObject {
    UUID.fromString(batch); identity(destination); check()
    require(scalar("SELECT uri FROM batches WHERE batch=?", batch) == null) { "Provider batch already exists" }
    val parent = document(Uri.parse(destination)); require(find(parent, name) == null) { "Destination batch already exists; no overwrite" }
    val uri = DC.createDocument(resolver, parent, DC.Document.MIME_TYPE_DIR, name) ?: error("Cannot create receive folder")
    // Probe creation/deletion before approval; provider output is visibly
    // Saving until close and readback. No rename that could replace a target.
    val probe = DC.createDocument(resolver, uri, "application/octet-stream", ".tenjee-probe-${UUID.randomUUID()}") ?: error("Provider cannot create files")
    try {
      val flags = metadata(probe).flags
      require(flags and DC.Document.FLAG_SUPPORTS_DELETE != 0) { "Provider cannot retain and discard incomplete files; choose a compatible local folder" }
    } finally { DC.deleteDocument(resolver, probe) }
    val stagingDirectory = File(privateRoot, "receive/$batch").apply { require(mkdirs()) { "Private batch already exists" } }
    require(File(stagingDirectory, ".tenjee-partials").mkdir()); syncDirectory(stagingDirectory); syncDirectory(stagingDirectory.parentFile!!)
    database.execSQL("INSERT INTO batches VALUES(?,?,?,?)", arrayOf(batch, uri.toString(), destination, stagingDirectory.absolutePath))
    database.execSQL("INSERT OR IGNORE INTO grants VALUES(?,?)", arrayOf(batch, destination))
    return JSObject().put("uri", uri.toString()).put("privatePath", stagingDirectory.absolutePath)
  }
  fun restoreBatch(uri: String): JSObject {
    val meta = metadata(Uri.parse(uri)); val stagingDirectory = scalar("SELECT private_path FROM batches WHERE uri=?", uri) ?: error("Provider recovery record unavailable")
    require(File(stagingDirectory).isDirectory) { "Private recovery files unavailable" }; return JSObject().put("privatePath", stagingDirectory).put("name", meta.name)
  }
  private fun batch(uri: String) = scalar("SELECT batch FROM batches WHERE uri=?", uri) ?: error("Unowned provider batch")
  private fun safePath(parts: List<String>) { require(parts.isNotEmpty() && parts.size <= 128); parts.forEach { require(it.isNotEmpty() && it != "." && it != ".." && !it.contains('/') && !it.contains('\\') && !it.contains('\u0000')) } }
  private fun directory(uri: String, parts: List<String>): Uri {
    val batch = batch(uri); var parent = Uri.parse(uri); var path = emptyList<String>()
    for (name in parts) {
      check(); safePath(listOf(name)); path = path + name; val key = JSONArray(path).toString()
      val known = scalar("SELECT uri FROM directories WHERE batch=? AND path=?", batch, key)
      if (known != null) { parent = Uri.parse(known); require(metadata(parent).mime == DC.Document.MIME_TYPE_DIR && metadata(parent).name == name) { "Saved destination folder changed" } }
      else {
        require(find(parent, name) == null) { "Unexpected destination entry; no merging or overwrite" }
        parent = DC.createDocument(resolver, parent, DC.Document.MIME_TYPE_DIR, name) ?: error("Cannot create receive subfolder")
        database.execSQL("INSERT INTO directories VALUES(?,?,?)", arrayOf(batch, key, parent.toString()))
      }
    }
    return parent
  }
  fun verifyDirectory(uri: String, path: List<String>) { safePath(path); val saved = scalar("SELECT uri FROM directories WHERE batch=? AND path=?", batch(uri), JSONArray(path).toString()) ?: error("Saved folder recovery record unavailable"); val meta = metadata(Uri.parse(saved)); require(meta.mime == DC.Document.MIME_TYPE_DIR && meta.name == path.last()) { "Saved destination folder changed" } }
  fun createDirectory(uri: String, path: List<String>) { safePath(path); directory(uri, path) }
  private fun digest(uri: Uri, expected: Long): String {
    val hash = MessageDigest.getInstance("SHA-256"); var total = 0L
    val source = resolver.openInputStream(uri) ?: error("Cannot verify saved file"); input.set(source)
    try { source.use { val buffer = ByteArray(65536); while (true) { check(); val n = it.read(buffer); if (n < 0) break; total = Math.addExact(total, n.toLong()); require(total <= expected); hash.update(buffer, 0, n); progressed() } } }
    finally { input.set(null) }
    require(total == expected) { "Saved file length differs" }; return hex(hash.digest())
  }
  fun publish(uri: String, id: Int, parts: List<String>, sourcePath: String, expected: Long) {
    safePath(parts); val batch = batch(uri); val stagingDirectory = File(scalar("SELECT private_path FROM batches WHERE batch=?", batch)!!).canonicalFile
    val source = File(sourcePath).canonicalFile
    require(source == File(stagingDirectory, ".tenjee-partials/$id.part") && source.isFile && source.length() == expected) { "Unowned private transfer file" }
    val parent = directory(uri, parts.dropLast(1)); val name = parts.last(); val key = JSONArray(parts).toString()
    val saved = scalar("SELECT saved FROM entries WHERE batch=? AND id=? AND state='complete'", batch, id.toString())
    if (saved != null) { require(digest(Uri.parse(saved), expected) == fileDigest(source)); return }
    val previous = scalar("SELECT temp FROM entries WHERE batch=? AND id=?", batch, id.toString())
    if (previous != null) DC.deleteDocument(resolver, Uri.parse(previous))
    require(find(parent, name) == null) { "Destination name already exists; no overwrite" }
    val temporary = DC.createDocument(resolver, parent, "application/octet-stream", name) ?: error("Cannot create provider temporary file")
    database.execSQL("INSERT OR REPLACE INTO entries VALUES(?,?,?,?,?,?)", arrayOf(batch, id, key, null, temporary.toString(), "saving"))
    val target = resolver.openOutputStream(temporary, "wt") ?: error("Cannot write provider file"); output.set(target)
    try {
      target.use { sink -> FileInputStream(source).use { inputFile -> input.set(inputFile); val buffer = ByteArray(65536); while (true) { check(); val n = inputFile.read(buffer); if (n < 0) break; sink.write(buffer, 0, n); progressed() } }; sink.flush() }
    } finally { input.set(null); output.set(null) }
    require(digest(temporary, expected) == fileDigest(source)) { "Provider readback integrity check failed" }
    // createDocument creates a new document; never rename over another name.
    // A provider-added suffix means a concurrent collision, not consent to a
    // different naming plan. Retain this owned incomplete document for cleanup.
    require(metadata(temporary).name == name) { "Provider changed the approved name; no existing file was overwritten" }
    database.execSQL("UPDATE entries SET saved=?,temp=NULL,state='complete' WHERE batch=? AND id=?", arrayOf(temporary.toString(), batch, id))
  }
  private fun fileDigest(file: File): String { val hash = MessageDigest.getInstance("SHA-256"); FileInputStream(file).use { val buffer = ByteArray(65536); while (true) { check(); val n = it.read(buffer); if (n < 0) break; hash.update(buffer, 0, n); progressed() } }; return hex(hash.digest()) }
  fun readSaved(uri: String, id: Int, parts: List<String>, expected: Long): JSObject {
    val batch = batch(uri); val key = JSONArray(parts).toString()
    val saved = scalar("SELECT saved FROM entries WHERE batch=? AND id=? AND path=? AND state='complete'", batch, id.toString(), key) ?: return JSObject().put("missing", true)
    val stagingDirectory = File(scalar("SELECT private_path FROM batches WHERE batch=?", batch)!!)
    val verification = File(stagingDirectory, ".tenjee-partials/verify.current")
    require(metadata(Uri.parse(saved)).name == parts.last()) { "Saved destination name changed; no overwrite" }
    copyToPrivate(Uri.parse(saved), verification, expected)
    val descriptor = ParcelFileDescriptor.open(verification, ParcelFileDescriptor.MODE_READ_ONLY)
    try { require(verification.delete()) { "Cannot release verification staging" }; return JSObject().put("fd", descriptor.detachFd()) }
    finally { descriptor.close() }
  }
  fun discard(batch: String) {
    UUID.fromString(batch); cancel()
    scans.values.filter { it.batch == batch }.forEach { scan -> scan.levels.forEach { it.cursor.close() } }
    scans.entries.removeAll { it.value.batch == batch }
    database.rawQuery("SELECT temp FROM entries WHERE batch=? AND temp IS NOT NULL", arrayOf(batch)).use { while (it.moveToNext()) DC.deleteDocument(resolver, Uri.parse(it.getString(0))) }
    // Never delete saved provider files or the user's chosen tree.
    File(privateRoot, "sources/$batch").deleteRecursively()
    scalar("SELECT private_path FROM batches WHERE batch=?", batch)?.let { File(it).deleteRecursively() }
    val grants = mutableListOf<String>(); database.rawQuery("SELECT uri FROM grants WHERE batch=?", arrayOf(batch)).use { while (it.moveToNext()) grants.add(it.getString(0)) }
    database.execSQL("DELETE FROM source_stage WHERE batch=?", arrayOf(batch)); database.execSQL("DELETE FROM entries WHERE batch=?", arrayOf(batch)); database.execSQL("DELETE FROM directories WHERE batch=?", arrayOf(batch)); database.execSQL("DELETE FROM batches WHERE batch=?", arrayOf(batch)); database.execSQL("DELETE FROM grants WHERE batch=?", arrayOf(batch))
    val backup = activity.getSharedPreferences("backup-documents", Activity.MODE_PRIVATE).getString("tree", null)
    for (grant in grants) if (grant != backup && scalar("SELECT uri FROM grants WHERE uri=? LIMIT 1", grant) == null) {
      val permission = resolver.persistedUriPermissions.find { it.uri.toString() == grant }
      val flags = (if (permission?.isReadPermission == true) Intent.FLAG_GRANT_READ_URI_PERMISSION else 0) or (if (permission?.isWritePermission == true) Intent.FLAG_GRANT_WRITE_URI_PERMISSION else 0)
      if (flags != 0) try { resolver.releasePersistableUriPermission(Uri.parse(grant), flags); roots.edit().remove(grant).commit() } catch (_: Exception) {}
    }
  }
}
