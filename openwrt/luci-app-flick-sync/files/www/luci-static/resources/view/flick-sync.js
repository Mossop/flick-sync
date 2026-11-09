'use strict';
'require view';
'require form';
'require uci';
'require rpc';
'require poll';
'require fs';

var STORE_FILE = '/var/run/flick-sync/store';

var callServiceList = rpc.declare({
	object: 'service',
	method: 'list',
	params: ['name'],
	expect: { '': {} }
});

function getServiceStatus() {
	return L.resolveDefault(callServiceList('flick-sync'), {}).then(function (res) {
		try {
			return res['flick-sync']['instances']['instance1']['running'] === true;
		} catch (e) {
			return false;
		}
	});
}

// The store the start script settled on, recorded for the hotplug handler.
function getActiveStore() {
	return L.resolveDefault(fs.read(STORE_FILE), '').then(function (content) {
		return (content || '').trim();
	});
}

function getStatus() {
	return Promise.all([getServiceStatus(), getActiveStore()]).then(function (results) {
		return { running: results[0], store: results[1] };
	});
}

function renderStatus(status, port) {
	if (!status.running) {
		return E('em', {}, _('Not running'));
	}

	var url = 'http://' + window.location.hostname + ':' + port + '/';
	var parts = [
		E('strong', { 'style': 'color: green' }, _('Running')),
		' \u2014 ',
		E('a', { 'href': url, 'target': '_blank' }, _('Open web interface'))
	];

	if (status.store) {
		parts.push(E('br'));
		parts.push(_('Store in use: '));
		parts.push(E('code', {}, status.store));
	}

	return E('span', {}, parts);
}

return view.extend({
	load: function () {
		return getStatus();
	},

	render: function (status) {
		var m, s, o;

		m = new form.Map('flick-sync', _('Flick Sync'),
			_('Synchronises media from a Plex Media Server to local storage and serves it over DLNA and a web interface.'));

		s = m.section(form.NamedSection, 'main', 'flick-sync');

		o = s.option(form.DummyValue, '_status', _('Status'));
		o.rawhtml = true;
		o.cfgvalue = function (section_id) {
			var port = uci.get('flick-sync', section_id, 'port') || '3000';
			var el = E('span', { 'id': 'flick-sync-status' }, renderStatus(status, port));

			poll.add(function () {
				return getStatus().then(function (current) {
					var node = document.getElementById('flick-sync-status');
					if (node) {
						while (node.firstChild)
							node.removeChild(node.firstChild);
						node.appendChild(renderStatus(current, port));
					}
				});
			}, 5);

			return el;
		};

		o = s.option(form.Flag, 'enabled', _('Enabled'),
			_('Start the server at boot.'));
		o.rmempty = false;
		o.default = '1';

		o = s.option(form.Value, 'store', _('Store directory'),
			_('Directory to keep synced media and state in. Leave blank to search mounted external storage for a directory containing a flicksync.json file, checking the root of each partition and one directory below it.'));
		o.datatype = 'directory';
		o.rmempty = true;
		o.placeholder = _('Search mounted storage');

		o = s.option(form.Value, 'port', _('Port'),
			_('Port for the web interface. Port 80 is used by the router administration pages.'));
		o.datatype = 'port';
		o.rmempty = false;
		o.default = '3000';

		o = s.option(form.Value, 'certificate', _('TLS certificate'),
			_('Optional path to a certificate (e.g. a .cer file, PEM or DER) to also serve the web interface over HTTPS.'));
		o.datatype = 'file';

		o = s.option(form.Value, 'private_key', _('TLS private key'),
			_('Path to the private key for the certificate above (e.g. a .key file, PEM or DER).'));
		o.datatype = 'file';
		o.depends({ certificate: /.+/ });

		o = s.option(form.Value, 'https_port', _('HTTPS port'),
			_('Port for the web interface over HTTPS. Port 443 is used by the router administration pages.'));
		o.datatype = 'port';
		o.placeholder = '3443';
		o.default = '3443';
		o.depends({ certificate: /.+/ });

		o = s.option(form.Flag, 'disable_syncing', _('Disable syncing'),
			_('Serve existing content without syncing from Plex.'));
		o.rmempty = false;
		o.default = '1';

		o = s.option(form.ListValue, 'log_level', _('Log level'),
			_('Only log messages at this level or above to the system log.'));
		o.value('error', _('Error'));
		o.value('warn', _('Warning'));
		o.value('info', _('Info'));
		o.value('debug', _('Debug'));
		o.value('trace', _('Trace'));
		o.default = 'warn';

		o = s.option(form.ListValue, 'mdns_zones', _('Cast devices'),
			_('Networks to allow cast devices on through the firewall while the server is running. Opens mDNS (UDP port 5353) to discover them and the web interface port so they can fetch media.'));
		o.value('lan', _('LAN only'));
		o.value('lan wan', _('LAN and WAN (private addresses only)'));
		o.default = 'lan';

		o = s.option(form.Value, 'hostname', _('Hostname'),
			_('Name in the local domain for reaching the web interface without a port, e.g. http://media.lan/. Only used when an address is set below.'));
		o.datatype = 'hostname';
		o.placeholder = 'media';
		o.default = 'media';

		o = s.option(form.Value, 'hostname_address', _('Hostname address'),
			_('An unused address in the LAN subnet, outside of the DHCP range. While the server runs it is added to the router, the hostname points to it, and ports 80 and, with HTTPS, 443 on it are forwarded to the web interface. Leave blank to not set up a hostname.'));
		o.datatype = 'ip4addr("nomask")';

		return m.render();
	}
});
