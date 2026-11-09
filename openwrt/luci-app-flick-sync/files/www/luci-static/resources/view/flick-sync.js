'use strict';
'require view';
'require form';
'require uci';
'require rpc';
'require poll';
'require fs';

// The store the start script settled on, recorded for the hotplug handler.
var STORE_FILE = '/var/run/flick-sync/store';
// Exists while the init script has set up the hostname's address.
var HOSTNAME_STATE_FILE = '/var/run/flick-sync/hostname';

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

function readFile(path) {
	return L.resolveDefault(fs.read(path), '').then(function (content) {
		return (content || '').trim();
	});
}

function getStatus() {
	return Promise.all([
		getServiceStatus(),
		readFile(STORE_FILE),
		readFile(HOSTNAME_STATE_FILE)
	]).then(function (results) {
		return { running: results[0], store: results[1], hostnameActive: results[2] != '' };
	});
}

function localDomain() {
	var dnsmasq = uci.sections('dhcp', 'dnsmasq')[0];
	return (dnsmasq && dnsmasq.domain) || 'lan';
}

// The addresses the web interface can be reached on, using the saved
// configuration.
function getUrls(status) {
	var get = function (option, fallback) {
		return uci.get('flick-sync', 'main', option) || fallback;
	};
	var https = get('certificate') && get('private_key');
	var hostname = get('hostname', 'media');
	var urls = [];

	if (status.hostnameActive) {
		var name = hostname + '.' + localDomain();
		urls.push('http://' + name + '/');
		if (https)
			urls.push('https://' + name + '/');
	}
	else {
		urls.push('http://' + window.location.hostname + ':' + get('port', '3000') + '/');
		if (https)
			urls.push('https://' + window.location.hostname + ':' + get('https_port', '3443') + '/');
	}

	return urls;
}

function renderStatus(status) {
	if (!status.running) {
		return E('em', {}, _('Not running'));
	}

	var parts = [E('strong', { 'style': 'color: green' }, _('Running'))];

	getUrls(status).forEach(function (url) {
		parts.push(' — ', E('a', { 'href': url, 'target': '_blank' }, url));
	});

	if (status.store) {
		parts.push(E('br'), _('Store in use: '), E('code', {}, status.store));
	}

	return E('span', {}, parts);
}

return view.extend({
	load: function () {
		return Promise.all([getStatus(), uci.load('dhcp')]).then(function (results) {
			return results[0];
		});
	},

	render: function (status) {
		var m, s, o;

		// The title, description and status are kept outside of the map,
		// which clears its contents whenever it re-renders, e.g. after saving.
		m = new form.Map('flick-sync');

		s = m.section(form.NamedSection, 'main', 'flick-sync');
		s.tab('general', _('General'));
		s.tab('web', _('Web interface'));
		s.tab('network', _('Network'));

		// General

		o = s.taboption('general', form.Flag, 'enabled', _('Enabled'),
			_('Run the server.'));
		o.rmempty = false;
		o.default = '1';

		o = s.taboption('general', form.Value, 'store', _('Store directory'),
			_('Directory to keep synced media and state in. Leave blank to search mounted external storage for a directory containing a flicksync.json file, checking the root of each partition and one directory below it.'));
		o.datatype = 'directory';
		o.rmempty = true;
		o.placeholder = _('Search mounted storage');

		// Stored inverted as disable_syncing.
		o = s.taboption('general', form.Flag, 'disable_syncing', _('Sync from Plex'),
			_('Download new content from Plex. When off, existing content is served only.'));
		o.enabled = '0';
		o.disabled = '1';
		o.default = '1';
		o.rmempty = false;

		o = s.taboption('general', form.ListValue, 'log_level', _('Log level'),
			_('Only log messages at this level or above to the system log.'));
		o.value('error', _('Error'));
		o.value('warn', _('Warning'));
		o.value('info', _('Info'));
		o.value('debug', _('Debug'));
		o.value('trace', _('Trace'));
		o.default = 'warn';

		// Web interface

		o = s.taboption('web', form.Value, 'port', _('Port'),
			_('Port for the web interface. Port 80 is used by the router administration pages.'));
		o.datatype = 'port';
		o.rmempty = false;
		o.default = '3000';

		o = s.taboption('web', form.Value, 'certificate', _('TLS certificate'),
			_('Optional path to a certificate (e.g. a .cer file, PEM or DER) to also serve the web interface over HTTPS. Changes to the file are picked up automatically.'));
		o.datatype = 'file';

		o = s.taboption('web', form.Value, 'private_key', _('TLS private key'),
			_('Path to the private key for the certificate (e.g. a .key file, PEM or DER).'));
		o.datatype = 'file';
		o.depends({ certificate: /.+/ });

		o = s.taboption('web', form.Value, 'https_port', _('HTTPS port'),
			_('Port for the web interface over HTTPS. Port 443 is used by the router administration pages.'));
		o.datatype = 'port';
		o.placeholder = '3443';
		o.default = '3443';
		o.depends({ certificate: /.+/ });

		// Network

		o = s.taboption('network', form.ListValue, 'mdns_zones', _('Cast devices'),
			_('Networks to allow cast devices on through the firewall while the server is running. Opens mDNS (UDP port 5353) to discover them and the web interface port so they can fetch media.'));
		o.value('lan', _('LAN only'));
		o.value('lan wan', _('LAN and WAN (private addresses only)'));
		o.default = 'lan';

		o = s.taboption('network', form.Value, 'hostname_address', _('Hostname address'),
			_('Set to reach the web interface on a name without a port, e.g. http://media.lan/. Use an unused address in the LAN subnet, outside of the DHCP range. While the server runs it is added to the router, the hostname points to it, and ports 80 and, with HTTPS, 443 on it are forwarded to the web interface.'));
		o.datatype = 'ip4addr("nomask")';

		o = s.taboption('network', form.Value, 'hostname', _('Hostname'),
			_('Name in the local domain (%s) for the web interface.').format(localDomain()));
		o.datatype = 'hostname';
		o.placeholder = 'media';
		o.default = 'media';
		o.depends({ hostname_address: /.+/ });

		var statusEl = E('div', { 'id': 'flick-sync-status' }, renderStatus(status));

		poll.add(function () {
			return getStatus().then(function (current) {
				while (statusEl.firstChild)
					statusEl.removeChild(statusEl.firstChild);
				statusEl.appendChild(renderStatus(current));
			});
		}, 5);

		return m.render().then(function (mapEl) {
			return E([], [
				E('h2', {}, _('Flick Sync')),
				E('div', { 'class': 'cbi-map-descr' },
					_('Synchronises media from a Plex Media Server to local storage and serves it over DLNA and a web interface.')),
				E('div', { 'class': 'cbi-section' }, [
					E('h3', {}, _('Status')),
					statusEl
				]),
				mapEl
			]);
		});
	}
});
