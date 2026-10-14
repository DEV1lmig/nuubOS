/* nuubOS: SSH/SCP/SFTP sign-in as root with the device credential
 * (nuubos-remotectl writes its hash to /etc/shadow) or with a public key
 * added from the Web administration (EPIC-039; nuubos-remotectl keys,
 * /root/.ssh/authorized_keys -> /state/ssh/root). */
#define DROPBEAR_SVR_PUBKEY_AUTH 1
