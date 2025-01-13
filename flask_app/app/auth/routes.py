from flask import Blueprint, redirect, url_for, flash, request
from werkzeug.datastructures import MultiDict
from flask_login import login_user, logout_user, current_user
from .. import ldap_manager, login_manager
from flask import current_app as app
from ..models import User

auth_bp = Blueprint('auth', __name__)

@login_manager.user_loader
def load_user(id):
    # Return a basic user instance with just the ID/DN
    # This will be enhanced by Flask-Login's session management
    return User(id=id, dn=id, username=None, gid=None, group=None)

@auth_bp.route('/login', methods=['POST'])
def login():
    username = request.form.get('username')
    password = request.form.get('password')

    try:
        # First authenticate the user
        auth_result = ldap_manager.authenticate(username, password)
        if not auth_result.status:
            flash('Invalid credentials')
            return redirect(url_for('main.index'))

        # # Bind as admin for searches
        # ldap_manager.connection.unbind()
        # ldap_manager.connection.bind(
        #     app.config['LDAP_ADMIN_DN'],
        #     app.config['LDAP_ADMIN_PASSWORD']
        # )

        # Get user details
        user_filter = f'(&(uid={username}){app.config["LDAP_USER_OBJECT_FILTER"]})'

        ldap_manager.connection.search(
            search_base=app.config["LDAP_USER_DN"],
            search_filter=user_filter,
            attributes=['cn', 'uid', 'gidNumber'],
            controls=[]
        )
        
        if not ldap_manager.connection.entries:
            flash('User details not found')
            return redirect(url_for('main.index'))

        username = ldap_manager.connection.entries[0].uid.values[0] if ldap_manager.connection.entries else None
        # Get group details
        gid = ldap_manager.connection.entries[0].gidNumber.values[0]
        group_filter = f'(&(gidNumber={gid}){app.config["LDAP_GROUP_OBJECT_FILTER"]})'
        ldap_manager.connection.search(
            search_base=app.config["LDAP_GROUP_DN"],
            search_filter=group_filter,
            attributes=['cn', 'gidNumber'],
            controls=[]
        )

        groupname = ldap_manager.connection.entries[0].cn.values[0] if ldap_manager.connection.entries else None

        # Create user object
        user = User(
            id=auth_result.user_dn,
            dn=auth_result.user_dn,
            username=username,
            gid=gid,
            group=groupname
        )
        login_user(user)
        flash(f'Logged in as {username}')
        print(f'Logged in as {username}')
        return redirect(url_for('main.index'))

    except Exception as e:
        #print traceback
        import traceback
        traceback.print_exc()
        flash(f'Login failed: {str(e)}')
        return redirect(url_for('main.index'))

@auth_bp.route('/logout')
def logout():
    logout_user()
    flash('You have been logged out.')
    return redirect(url_for('main.index'))
