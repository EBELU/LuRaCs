import asyncio
import logging
import signal
from radiacode import RadiacodeAsync

# ---------------- Logging ----------------

logging.basicConfig(level=logging.INFO, format='%(asctime)s [%(levelname)s] %(name)s: %(message)s')

# ---------------- Device ----------------

DEVICE_MAC = '52:43:09:90:06:52'

import usb.core
import usb.util


def scan_all_usb():
    devices = usb.core.find(find_all=True)
    results = []

    for dev in devices or []:
        try:
            try:
                dev.set_configuration()
            except usb.core.USBError:
                pass

            results.append(
                {
                    'vendor_id': hex(dev.idVendor),
                    'product_id': hex(dev.idProduct),
                    'serial_number': usb.util.get_string(dev, dev.iSerialNumber),
                    'manufacturer': usb.util.get_string(dev, dev.iManufacturer),
                    'product': usb.util.get_string(dev, dev.iProduct),
                    'bus': getattr(dev, 'bus', None),
                    'address': getattr(dev, 'address', None),
                }
            )
        except Exception:
            # Skip devices we can't access
            continue

    return results


async def poll_device(rc: RadiacodeAsync):
    while True:
        try:
            spec = rc.LatestSpectrum
            cps = rc.LatestRealTimeData

            print('CPS:', cps)
            print('Spectrum:', spec)
            print('-' * 50)

        except Exception as e:
            logging.error(f'Polling error: {e}')

        await asyncio.sleep(0.5)


async def main():
    logging.info('Starting debug session')
    usb_d = scan_all_usb()
    print('USB found:', usb_d)
    rc = RadiacodeAsync(DEVICE_MAC)

    logging.info('Connecting to device...')
    await rc.start()
    logging.info('Connected.')

    poll_task = asyncio.create_task(poll_device(rc))

    # Wait forever until cancelled
    try:
        await asyncio.Event().wait()
    finally:
        logging.info('Stopping device...')
        await rc.stop()
        poll_task.cancel()
        logging.info('Shutdown complete.')


if __name__ == '__main__':
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        print('\nStopped by user')
