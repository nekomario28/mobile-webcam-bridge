package dev.nekomario.amb

import android.app.Activity
import android.app.AlertDialog
import android.view.View
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView
import java.text.DateFormat
import java.util.Date

/** The visible request keeps its exact session reference: a late tap must not
 * approve an unseen replacement connection. Approving never starts Camera2. */
class ClientApprovalUi(
    private val activity: Activity,
    private val owner: () -> PhoneAuthorization?,
    private val current: () -> MediaSession?,
    private val onError: () -> Unit,
    private val ui: (String, String) -> String,
) {
    private var request: PhoneAuthorization.Session? = null
    private val message = TextView(activity).apply {
        id = R.id.client_request
        textSize = 15f
        setTextIsSelectable(true)
    }
    private val panel = LinearLayout(activity).apply {
        orientation = LinearLayout.VERTICAL
        addView(message)
        visibility = View.GONE
    }
    val root = LinearLayout(activity).apply {
        orientation = LinearLayout.VERTICAL
        addView(panel)
        addView(Button(activity).apply {
            id = R.id.authorized_clients
            text = ui("Authorized PCs", "許可したPC")
            setAllCaps(false)
            setOnClickListener { showClients() }
        })
    }

    fun refresh() {
        val session = current()?.clientAuthorization
        val pending = session?.pending()
        val next = session?.takeIf { pending != null }
        if (request !== next) {
            request = next
            // Replace controls, not their targets: a queued click on an old
            // view must remain bound to that old session, never a replacement.
            while (panel.childCount > 1) panel.removeViewAt(1)
            if (next != null && pending != null) {
                message.text = ui(
                    "PC requests access\nProposed name (not verified): ${pending.label}\nPC ID: ${id(pending)}\n\nAllow only your PC. The camera still needs Start camera. Request expires after 60 seconds.",
                    "PCが接続を要求しています\nPCが申告した名前（未確認）: ${pending.label}\nPC ID: ${id(pending)}\n\n自分のPCだけ許可してください。撮影は別途「カメラ開始」が必要です。要求は60秒で失効します。")
                panel.addView(actionButton(R.id.allow_client, ui("Allow for 90 days", "90日間許可")) { next.approve() })
                panel.addView(actionButton(R.id.deny_client, ui("Reject", "拒否")) { next.reject() })
            }
        }
        panel.visibility = if (pending == null) View.GONE else View.VISIBLE
    }

    private fun actionButton(viewId: Int, label: String, action: () -> Unit) = Button(activity).apply {
        id = viewId
        text = label
        setAllCaps(false)
        setOnClickListener { act(action) }
    }

    private fun act(action: () -> Unit) {
        try { action() } catch (_: PhoneAuthorization.RequestEnded) {
            // A stale gesture is not a storage failure and cannot stop a new peer.
        } catch (_: Exception) { onError() }
        refresh()
    }

    private fun showClients() {
        runCatching {
            val clients = checkNotNull(owner()).clients()
            val dialog = AlertDialog.Builder(activity).setTitle(ui("Authorized PCs", "許可したPC"))
                .setNegativeButton(ui("Close", "閉じる"), null)
            if (clients.isEmpty()) dialog.setMessage(ui("No PCs authorized.", "許可したPCはありません。"))
            else dialog.setItems(clients.map { "${it.label}\n${expiry(it)}" }.toTypedArray()) { _, index -> showClient(clients[index]) }
            dialog.show()
        }.onFailure { onError() }
    }

    private fun showClient(client: ClientAuthorization.Client) {
        AlertDialog.Builder(activity)
            .setTitle(client.label)
            .setMessage("PC ID: ${id(client)}\n${expiry(client)}")
            .setNegativeButton(ui("Close", "閉じる"), null)
            .setNeutralButton(ui("Revoke", "許可を取り消す")) { _, _ ->
                confirm(ui("Revoke this PC? Its active connection will close.", "このPCの許可を取り消しますか？ 接続中なら切断します。")) {
                    checkNotNull(owner()).revoke(client.clientId)
                }
            }
            .setPositiveButton(ui("Renew 90 days", "90日間更新")) { _, _ ->
                confirm(ui("Renew this PC for 90 days from now?", "このPCの期限を今から90日間に更新しますか？")) {
                    checkNotNull(owner()).renew(client.clientId)
                }
            }.show()
    }

    private fun confirm(message: String, action: () -> Unit) {
        AlertDialog.Builder(activity).setMessage(message)
            .setNegativeButton(ui("Cancel", "キャンセル"), null)
            .setPositiveButton(ui("Confirm", "確定")) { _, _ -> act(action) }.show()
    }

    private fun id(client: ClientAuthorization.Client) = client.clientId.joinToString("") { "%02x".format(it) }.chunked(4).joinToString(" ")
    private fun expiry(client: ClientAuthorization.Client): String {
        val date = DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT)
            .format(Date(client.expiresAtUnixSeconds * 1000))
        return ui("Expires: $date", "期限: $date")
    }
}
