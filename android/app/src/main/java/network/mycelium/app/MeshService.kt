// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
package network.mycelium.app

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.os.IBinder
import androidx.core.app.NotificationCompat
import network.mycelium.bindings.BulletinPost
import network.mycelium.bindings.ChatMessage
import network.mycelium.bindings.MailMessage
import network.mycelium.bindings.NodeConfig
import network.mycelium.bindings.ConnectivityMode
import network.mycelium.bindings.NodeEventCallback
import network.mycelium.bindings.mycelium
import java.io.File

class MeshService : Service() {
    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        startForeground(NOTIFICATION_ID, buildNotification())
        val dbPath = filesDir.absolutePath + "/mycelium"
        File(dbPath).mkdirs()
        mycelium.initNode(
            NodeConfig(
                dbPath = dbPath,
                listenAddr = "/ip4/0.0.0.0/tcp/7761",
                displayName = "android-user",
                bootstrapPeers = emptyList(),
            ),
        )
        mycelium.setEventCallback(
            object : NodeEventCallback {
                override fun onPeerDiscovered(peerId: String) {
                    sendBroadcast(Intent("mycelium.PEER_UP").putExtra("peer_id", peerId))
                }

                override fun onPeerLost(peerId: String) {
                    sendBroadcast(Intent("mycelium.PEER_DOWN").putExtra("peer_id", peerId))
                }

                override fun onChatReceived(message: ChatMessage) {
                    showChatNotification(message)
                    sendBroadcast(Intent("mycelium.CHAT_RECEIVED"))
                }

                override fun onMailReceived(message: MailMessage) {
                    showMailNotification(message)
                    sendBroadcast(Intent("mycelium.MAIL_RECEIVED"))
                }

                override fun onBulletinReceived(post: BulletinPost) {
                    sendBroadcast(Intent("mycelium.BULLETIN_RECEIVED"))
                }

                override fun onConnectivityChanged(mode: ConnectivityMode) {
                    sendBroadcast(
                        Intent("mycelium.CONNECTIVITY").putExtra(
                            "internet",
                            mode == ConnectivityMode.INTERNET,
                        ),
                    )
                }
            },
        )
        return START_STICKY
    }

    private fun buildNotification(): Notification {
        val channel = NotificationChannel(
            "mycelium_bg",
            "Mycelium Netzwerk",
            NotificationManager.IMPORTANCE_LOW,
        )
        getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
        return NotificationCompat.Builder(this, "mycelium_bg")
            .setContentTitle("Mycelium aktiv")
            .setContentText("Mesh-Netzwerk läuft")
            .setSmallIcon(android.R.drawable.stat_notify_sync)
            .setOngoing(true)
            .build()
    }

    private fun showChatNotification(message: ChatMessage) {
        val manager = getSystemService(NotificationManager::class.java)
        val pendingIntent = PendingIntent.getActivity(
            this,
            1001,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val notification = NotificationCompat.Builder(this, "mycelium_bg")
            .setContentTitle("Neue Chat-Nachricht")
            .setContentText("${message.fromDisplayName}: ${message.body}")
            .setSmallIcon(android.R.drawable.stat_notify_chat)
            .setAutoCancel(true)
            .setContentIntent(pendingIntent)
            .build()
        manager.notify(1001, notification)
    }

    private fun showMailNotification(message: MailMessage) {
        val manager = getSystemService(NotificationManager::class.java)
        val pendingIntent = PendingIntent.getActivity(
            this,
            1002,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val notification = NotificationCompat.Builder(this, "mycelium_bg")
            .setContentTitle("Neue Mail: ${message.subject}")
            .setContentText("Von ${message.fromDisplayName}")
            .setSmallIcon(android.R.drawable.stat_notify_more)
            .setAutoCancel(true)
            .setContentIntent(pendingIntent)
            .build()
        manager.notify(1002, notification)
    }

    override fun onDestroy() {
        mycelium.stopNode()
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    companion object {
        private const val NOTIFICATION_ID = 7761
    }
}
