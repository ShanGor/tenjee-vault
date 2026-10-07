package com.sam.tenjee_vault

import android.app.AlarmManager
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import app.tauri.annotation.InvokeArg
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import org.json.JSONArray
import org.json.JSONObject
import java.text.SimpleDateFormat
import java.util.Locale

@InvokeArg
class NativeReminder {
  lateinit var key: String
  lateinit var entityKind: String
  lateinit var entityId: String
  var occurrenceKey = ""
  var slot = 0L
  lateinit var title: String
  lateinit var body: String
  lateinit var at: String
  var whenMs = 0L
}

object ReminderAlarms {
  private const val CHANNEL = "vault-reminders"
  private fun prefs(context: Context) = context.getSharedPreferences("reminder-alarms", Context.MODE_PRIVATE)
  private fun alarm(context: Context) = context.getSystemService(AlarmManager::class.java)
  private fun pending(context: Context, key: String): PendingIntent = PendingIntent.getBroadcast(
    context, 0,
    Intent(context, ReminderAlarmReceiver::class.java).setAction("${context.packageName}.REMINDER").setData(Uri.parse("tenjee-reminder://$key")),
    PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
  )
  private fun exact(context: Context) = Build.VERSION.SDK_INT < 31 || alarm(context).canScheduleExactAlarms()
  private fun allowed(context: Context) = NotificationManagerCompat.from(context).areNotificationsEnabled() &&
    (Build.VERSION.SDK_INT < 33 || ContextCompat.checkSelfPermission(context, android.Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED)

  private fun schedule(context: Context, record: JSONObject) {
    if (!allowed(context)) return
    if (record.getLong("whenMs") < 0) return
    val timestamp = record.getLong("whenMs").coerceAtLeast(System.currentTimeMillis() + 1000)
    val operation = pending(context, record.getString("key"))
    if (exact(context)) {
      try { alarm(context).setExactAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, timestamp, operation); return }
      catch (_: SecurityException) { /* Permission may be revoked between the check and scheduling. */ }
    }
    alarm(context).setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, timestamp, operation)
  }

  @Synchronized
  fun replace(context: Context, reminders: Array<NativeReminder>, horizon: Long): JSObject {
    require(reminders.size <= 256)
    val store = prefs(context)
    val old = JSONArray(store.getString("plan", "[]") ?: "[]")
    val fired = JSONObject(store.getString("fired", "{}") ?: "{}")
    val next = JSONArray()
    for (item in reminders) {
      require(item.key.matches(Regex("[a-f0-9]{64}")))
      if (fired.has(item.key)) continue
      next.put(JSONObject().put("key", item.key).put("entity_kind", item.entityKind).put("entity_id", item.entityId)
        .put("occurrence_key", item.occurrenceKey).put("slot", item.slot).put("title", item.title)
        .put("body", item.body).put("at", item.at).put("whenMs", item.whenMs))
    }
    // Persist the desired plan before changing OS alarms. Startup/boot can replay
    // this plan if the process exits between these steps.
    check(store.edit().putString("plan", next.toString()).putLong("horizon", horizon).commit())
    for (i in 0 until old.length()) alarm(context).cancel(pending(context, old.getJSONObject(i).getString("key")))
    for (i in 0 until next.length()) schedule(context, next.getJSONObject(i))
    return status(context)
  }

  @Synchronized
  fun restore(context: Context, recalculateTime: Boolean) {
    val store = prefs(context)
    val plan = JSONArray(store.getString("plan", "[]") ?: "[]")
    val fired = JSONObject(store.getString("fired", "{}") ?: "{}")
    val format = SimpleDateFormat("yyyy-MM-dd'T'HH:mm:ss", Locale.US).apply { isLenient = false }
    for (i in 0 until plan.length()) {
      val record = plan.getJSONObject(i)
      if (recalculateTime) {
        try { format.parse(record.getString("at"))?.let { record.put("whenMs", it.time) } }
        catch (_: Exception) { record.put("whenMs", -1L) }
      }
    }
    check(store.edit().putString("plan", plan.toString()).commit())
    for (i in 0 until plan.length()) {
      val record = plan.getJSONObject(i)
      alarm(context).cancel(pending(context, record.getString("key")))
      if (!fired.has(record.getString("key"))) schedule(context, record)
    }
  }

  @Synchronized
  fun fire(context: Context, key: String) {
    val store = prefs(context)
    val fired = JSONObject(store.getString("fired", "{}") ?: "{}")
    if (fired.has(key)) return
    val plan = JSONArray(store.getString("plan", "[]") ?: "[]")
    val record = (0 until plan.length()).map { plan.getJSONObject(it) }.firstOrNull { it.getString("key") == key } ?: return
    if (record.getLong("whenMs") < 0) return
    // Revoked permission does not consume the reminder: resuming after a grant
    // schedules it again. The foreground shows the denied-permission status.
    if (!allowed(context)) return
    val manager = context.getSystemService(NotificationManager::class.java)
    if (Build.VERSION.SDK_INT >= 26) manager.createNotificationChannel(NotificationChannel(CHANNEL, "Tenjee Vault reminders", NotificationManager.IMPORTANCE_DEFAULT))
    val launch = Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP)
    val tap = PendingIntent.getActivity(context, key.hashCode(), launch, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
    val notification = NotificationCompat.Builder(context, CHANNEL).setSmallIcon(android.R.drawable.ic_popup_reminder)
      .setContentTitle(record.getString("title")).setContentText(record.getString("body"))
      .setStyle(NotificationCompat.BigTextStyle().bigText(record.getString("body")))
      .setContentIntent(tap).setAutoCancel(true).setOnlyAlertOnce(true).build()
    try {
      manager.notify(key, 0, notification)
      fired.put(key, record)
      check(store.edit().putString("fired", fired.toString()).commit())
    } catch (_: SecurityException) { }
  }

  @Synchronized
  fun status(context: Context): JSObject {
    val store = prefs(context)
    val plan=JSONArray(store.getString("plan", "[]") ?: "[]")
    val fired=JSONObject(store.getString("fired", "{}") ?: "{}")
    val scheduled=(0 until plan.length()).count { val record=plan.getJSONObject(it);record.getLong("whenMs")>=0 && !fired.has(record.getString("key")) }
    return JSObject().put("supported", true).put("permissionGranted", allowed(context))
      .put("exact", exact(context)).put("scheduled", if (allowed(context)) scheduled else 0)
      .put("horizon", store.getLong("horizon", 0))
  }

  @Synchronized
  fun drainFires(context: Context): JSObject {
    // Retain identities until the next plan excludes them. Reading is safe to
    // repeat if Rust exits before persisting reminder_fires.
    val fired = JSONObject(prefs(context).getString("fired", "{}") ?: "{}")
    val array = JSArray()
    fired.keys().forEach { key -> array.put(fired.getJSONObject(key)) }
    return JSObject().put("fires", array)
  }

  @Synchronized
  fun ackFires(context: Context, keys: Array<String>) {
    val store = prefs(context)
    val fired = JSONObject(store.getString("fired", "{}") ?: "{}")
    keys.forEach { fired.remove(it) }
    check(store.edit().putString("fired", fired.toString()).commit())
  }
}

class ReminderAlarmReceiver : BroadcastReceiver() {
  override fun onReceive(context: Context, intent: Intent) {
    val pending = goAsync()
    Thread {
      try { intent.data?.host?.let { ReminderAlarms.fire(context, it) } }
      finally { pending.finish() }
    }.start()
  }
}

class ReminderBootReceiver : BroadcastReceiver() {
  override fun onReceive(context: Context, intent: Intent) {
    val pending = goAsync()
    Thread {
      try { ReminderAlarms.restore(context, intent.action == Intent.ACTION_TIMEZONE_CHANGED || intent.action == Intent.ACTION_TIME_CHANGED) }
      finally { pending.finish() }
    }.start()
  }
}
